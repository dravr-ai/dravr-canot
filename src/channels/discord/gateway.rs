// ABOUTME: Discord Gateway WebSocket client for receiving real-time message events
// ABOUTME: Connects to wss://gateway.discord.gg, maintains heartbeat, emits IncomingMessage on MESSAGE_CREATE
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::error::Error as StdError;
use std::time::Duration;

use chrono::Utc;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep, timeout, Interval};
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::models::{ChannelType, IncomingMessage, MessageContent};

/// Discord Gateway opcodes
const OPCODE_DISPATCH: u64 = 0;
const OPCODE_HEARTBEAT: u64 = 1;
const OPCODE_IDENTIFY: u64 = 2;
const OPCODE_RECONNECT: u64 = 7;
const OPCODE_INVALID_SESSION: u64 = 9;
const OPCODE_HELLO: u64 = 10;
const OPCODE_HEARTBEAT_ACK: u64 = 11;

/// Discord Gateway intents bitmask
/// `GUILDS` (1 << 0) | `GUILD_MESSAGES` (1 << 9) | `MESSAGE_CONTENT` (1 << 15) | `DIRECT_MESSAGES` (1 << 12)
const GATEWAY_INTENTS: u64 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 15);

/// Discord Gateway URL
const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

/// Maximum consecutive reconnect attempts before giving up
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Base delay between reconnect attempts (exponential backoff)
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);

/// Maximum delay between reconnect attempts
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);

/// Configuration for the Discord Gateway client
#[derive(Clone)]
pub struct GatewayConfig {
    /// Discord bot token for authentication
    pub bot_token: String,
    /// Bot's own user ID (populated after READY event)
    bot_user_id: Option<String>,
}

impl GatewayConfig {
    /// Create a new Gateway configuration
    #[must_use]
    pub fn new(bot_token: String) -> Self {
        Self {
            bot_token,
            bot_user_id: None,
        }
    }
}

/// Start the Discord Gateway client
///
/// Connects to Discord's WebSocket Gateway, authenticates with the bot token,
/// maintains a heartbeat, and forwards `MESSAGE_CREATE` events as `IncomingMessage`
/// through the provided channel sender.
///
/// Automatically reconnects on disconnection with exponential backoff.
/// Returns when the sender is dropped or after exhausting reconnect attempts.
pub async fn start_gateway(config: GatewayConfig, tx: mpsc::Sender<IncomingMessage>) {
    let mut consecutive_failures: u32 = 0;
    let mut session_config = config;

    loop {
        let outcome = run_gateway_session(&mut session_config, &tx).await;

        // Receiver dropped means no one is listening — exit immediately
        if matches!(outcome.reason, DisconnectReason::ReceiverDropped) {
            info!("Discord Gateway: receiver dropped, stopping");
            return;
        }

        // Reset failure counter when session was established (reached READY)
        if outcome.reached_ready {
            consecutive_failures = 0;
        } else {
            consecutive_failures += 1;
        }

        if consecutive_failures > MAX_RECONNECT_ATTEMPTS {
            error!(
                attempts = consecutive_failures,
                "Discord Gateway: max reconnect attempts exhausted, stopping"
            );
            return;
        }

        let delay = calculate_backoff_delay(consecutive_failures);
        warn!(
            attempt = consecutive_failures,
            delay_secs = delay.as_secs(),
            reason = ?outcome.reason,
            reached_ready = outcome.reached_ready,
            "Discord Gateway: disconnected, reconnecting"
        );
        sleep(delay).await;
    }
}

/// Outcome of a single Gateway session
#[derive(Debug)]
struct SessionOutcome {
    /// Why the session ended
    reason: DisconnectReason,
    /// Whether the session reached READY state (authenticated and processing messages)
    reached_ready: bool,
}

/// Reason for Gateway session disconnection
#[derive(Debug)]
enum DisconnectReason {
    /// WebSocket connection lost
    ConnectionLost,
    /// Receiver dropped (no one listening for messages)
    ReceiverDropped,
}

/// Run a single Gateway WebSocket session until disconnection
async fn run_gateway_session(
    config: &mut GatewayConfig,
    tx: &mpsc::Sender<IncomingMessage>,
) -> SessionOutcome {
    let not_ready = |reason| SessionOutcome {
        reason,
        reached_ready: false,
    };

    let (ws_stream, _) = match connect_async(GATEWAY_URL).await {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = %e, "Discord Gateway: WebSocket connection failed");
            return not_ready(DisconnectReason::ConnectionLost);
        }
    };

    info!("Discord Gateway: connected");

    let (mut write, mut read) = ws_stream.split();

    // Wait for HELLO to get heartbeat interval
    let Some(heartbeat_interval_ms) = read_hello(&mut read).await else {
        error!("Discord Gateway: did not receive HELLO");
        return not_ready(DisconnectReason::ConnectionLost);
    };

    debug!(heartbeat_interval_ms, "Discord Gateway: received HELLO");

    // Send IDENTIFY
    let identify_payload = json!({
        "op": OPCODE_IDENTIFY,
        "d": {
            "token": config.bot_token,
            "intents": GATEWAY_INTENTS,
            "properties": {
                "os": "linux",
                "browser": "dravr-canot",
                "device": "dravr-canot"
            }
        }
    });

    if let Err(e) = write
        .send(WsMessage::Text(identify_payload.to_string()))
        .await
    {
        error!(error = %e, "Discord Gateway: failed to send IDENTIFY");
        return not_ready(DisconnectReason::ConnectionLost);
    }

    // Event loop: heartbeat + dispatch
    let mut heartbeat_timer = interval(Duration::from_millis(heartbeat_interval_ms));
    let mut last_sequence: Option<u64> = None;
    let mut heartbeat_ack_pending = false;

    let reason = run_event_loop(
        &mut write,
        &mut read,
        &mut heartbeat_timer,
        &mut last_sequence,
        &mut heartbeat_ack_pending,
        config,
        tx,
    )
    .await;

    // Session reached READY if bot_user_id was populated during this session
    SessionOutcome {
        reason,
        reached_ready: config.bot_user_id.is_some(),
    }
}

/// Run the main event loop for heartbeat and message dispatch
///
/// Extracted to reduce function line count. Handles all incoming WebSocket messages
/// and timer-based heartbeat sending.
async fn run_event_loop<S>(
    write: &mut SplitSink<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>,
    read: &mut S,
    heartbeat_timer: &mut Interval,
    last_sequence: &mut Option<u64>,
    heartbeat_ack_pending: &mut bool,
    config: &mut GatewayConfig,
    tx: &mpsc::Sender<IncomingMessage>,
) -> DisconnectReason
where
    S: StreamExt<Item = Result<WsMessage, TungsteniteError>> + Unpin,
{
    loop {
        tokio::select! {
            _ = heartbeat_timer.tick() => {
                if *heartbeat_ack_pending {
                    warn!("Discord Gateway: missed heartbeat ACK, reconnecting");
                    return DisconnectReason::ConnectionLost;
                }

                let heartbeat = json!({
                    "op": OPCODE_HEARTBEAT,
                    "d": last_sequence
                });

                if let Err(e) = write.send(WsMessage::Text(heartbeat.to_string())).await {
                    error!(error = %e, "Discord Gateway: failed to send heartbeat");
                    return DisconnectReason::ConnectionLost;
                }

                *heartbeat_ack_pending = true;
                debug!("Discord Gateway: sent heartbeat");
            }

            msg = read.next() => {
                if let Some(reason) = handle_incoming_message(
                    msg,
                    write,
                    last_sequence,
                    heartbeat_ack_pending,
                    config,
                    tx,
                )
                .await
                {
                    return reason;
                }
            }
        }
    }
}

/// Handle a single incoming WebSocket message
///
/// Returns `None` to continue the event loop, `Some(reason)` to disconnect.
async fn handle_incoming_message<S>(
    msg: Option<Result<WsMessage, S>>,
    write: &mut SplitSink<tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>,
    last_sequence: &mut Option<u64>,
    heartbeat_ack_pending: &mut bool,
    config: &mut GatewayConfig,
    tx: &mpsc::Sender<IncomingMessage>,
) -> Option<DisconnectReason>
where
    S: StdError,
{
    match msg {
        Some(Ok(WsMessage::Text(text))) => {
            let payload: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error = %e, "Discord Gateway: invalid JSON");
                    return None;
                }
            };

            let op = payload.get("op").and_then(Value::as_u64).unwrap_or(0);

            // Track sequence number for heartbeats
            if let Some(seq) = payload.get("s").and_then(Value::as_u64) {
                *last_sequence = Some(seq);
            }

            match op {
                OPCODE_DISPATCH => {
                    let event_name = payload.get("t").and_then(Value::as_str).unwrap_or("");

                    match event_name {
                        "READY" => {
                            // Extract bot user ID from READY
                            if let Some(user_id) =
                                payload.pointer("/d/user/id").and_then(Value::as_str)
                            {
                                config.bot_user_id = Some(user_id.to_owned());
                                info!(bot_user_id = %user_id, "Discord Gateway: READY");
                            } else {
                                info!("Discord Gateway: READY (no user ID)");
                            }
                        }
                        "MESSAGE_CREATE" => {
                            if let Some(data) = payload.get("d") {
                                if let Some(msg) =
                                    parse_message_create(data, config.bot_user_id.as_deref())
                                {
                                    if tx.send(msg).await.is_err() {
                                        info!("Discord Gateway: message receiver dropped");
                                        return Some(DisconnectReason::ReceiverDropped);
                                    }
                                }
                            }
                        }
                        _ => {
                            debug!(event = event_name, "Discord Gateway: unhandled event");
                        }
                    }
                }
                OPCODE_HEARTBEAT_ACK => {
                    *heartbeat_ack_pending = false;
                    debug!("Discord Gateway: heartbeat ACK");
                }
                OPCODE_HEARTBEAT => {
                    // Server-requested heartbeat: send one immediately
                    let heartbeat = json!({
                        "op": OPCODE_HEARTBEAT,
                        "d": last_sequence
                    });
                    if let Err(e) = write.send(WsMessage::Text(heartbeat.to_string())).await {
                        error!(error = %e, "Discord Gateway: failed to send requested heartbeat");
                        return Some(DisconnectReason::ConnectionLost);
                    }
                }
                OPCODE_RECONNECT => {
                    info!("Discord Gateway: server requested reconnect");
                    return Some(DisconnectReason::ConnectionLost);
                }
                OPCODE_INVALID_SESSION => {
                    warn!("Discord Gateway: invalid session");
                    // Wait a bit before reconnecting as Discord recommends
                    sleep(Duration::from_secs(3)).await;
                    return Some(DisconnectReason::ConnectionLost);
                }
                _ => {
                    debug!(op, "Discord Gateway: unhandled opcode");
                }
            }

            None
        }
        Some(Ok(WsMessage::Close(frame))) => {
            let code = frame.as_ref().map(|f| f.code);
            warn!(?code, "Discord Gateway: WebSocket closed");
            Some(DisconnectReason::ConnectionLost)
        }
        Some(Err(e)) => {
            error!(error = %e, "Discord Gateway: WebSocket error");
            Some(DisconnectReason::ConnectionLost)
        }
        None => {
            warn!("Discord Gateway: WebSocket stream ended");
            Some(DisconnectReason::ConnectionLost)
        }
        _ => None,
    }
}

/// Read the HELLO message and extract heartbeat interval
async fn read_hello<S>(read: &mut S) -> Option<u64>
where
    S: StreamExt<Item = Result<WsMessage, TungsteniteError>> + Unpin,
{
    let hello_timeout = Duration::from_secs(10);

    match timeout(hello_timeout, read.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            let payload: Value = serde_json::from_str(&text).ok()?;
            let op = payload.get("op").and_then(Value::as_u64)?;
            if op != OPCODE_HELLO {
                return None;
            }
            payload
                .pointer("/d/heartbeat_interval")
                .and_then(Value::as_u64)
        }
        _ => None,
    }
}

/// Parse a `MESSAGE_CREATE` dispatch event into an `IncomingMessage`
///
/// Skips messages from the bot itself to prevent self-reply loops.
fn parse_message_create(data: &Value, bot_user_id: Option<&str>) -> Option<IncomingMessage> {
    // Skip messages from bots (including ourselves)
    let author = data.get("author")?;
    let is_bot = author.get("bot").and_then(Value::as_bool).unwrap_or(false);
    if is_bot {
        return None;
    }

    let author_id = author.get("id").and_then(Value::as_str)?;

    // Extra safety: skip if author is the bot itself
    if bot_user_id.is_some_and(|bot_id| bot_id == author_id) {
        return None;
    }

    let content = data.get("content").and_then(Value::as_str).unwrap_or("");
    if content.is_empty() {
        return None;
    }

    let username = author
        .get("username")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let channel_id = data.get("channel_id").and_then(Value::as_str).unwrap_or("");
    let message_id = data.get("id").and_then(Value::as_str).unwrap_or("0");

    Some(IncomingMessage {
        channel_type: ChannelType::Discord,
        sender_id: author_id.to_owned(),
        sender_name: username,
        content: MessageContent::Text {
            body: content.to_owned(),
        },
        conversation_id: Some(channel_id.to_owned()),
        channel_message_id: message_id.to_owned(),
        timestamp: Utc::now(),
        raw_payload: data.clone(),
        correlation_id: Uuid::new_v4(),
        metadata: Value::Null,
    })
}

/// Calculate exponential backoff delay with jitter
fn calculate_backoff_delay(attempt: u32) -> Duration {
    let base_ms = RECONNECT_BASE_DELAY.as_millis() as u64;
    let max_ms = RECONNECT_MAX_DELAY.as_millis() as u64;
    let delay_ms = base_ms.saturating_mul(1u64 << attempt.min(31)).min(max_ms);
    Duration::from_millis(delay_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_create_skips_bot_messages() {
        let data = json!({
            "id": "123",
            "channel_id": "456",
            "content": "hello",
            "author": {
                "id": "789",
                "username": "TestBot",
                "bot": true
            }
        });

        assert!(parse_message_create(&data, None).is_none());
    }

    #[test]
    fn parse_message_create_skips_own_messages() {
        let data = json!({
            "id": "123",
            "channel_id": "456",
            "content": "hello",
            "author": {
                "id": "bot-id-123",
                "username": "Pierre"
            }
        });

        assert!(parse_message_create(&data, Some("bot-id-123")).is_none());
    }

    #[test]
    fn parse_message_create_returns_user_message() {
        let data = json!({
            "id": "msg-1",
            "channel_id": "ch-1",
            "content": "Yo Pierre",
            "author": {
                "id": "user-42",
                "username": "Jeanfranc"
            }
        });

        let msg = parse_message_create(&data, Some("bot-id-123"))
            .expect("parse_message_create should return a message for valid user input"); // Safe: test data is well-formed
        assert_eq!(msg.sender_id, "user-42");
        assert_eq!(msg.sender_name.as_deref(), Some("Jeanfranc"));
        assert_eq!(msg.conversation_id.as_deref(), Some("ch-1"));
        assert_eq!(msg.channel_message_id, "msg-1");
        assert!(
            matches!(&msg.content, MessageContent::Text { body } if body == "Yo Pierre"),
            "expected Text content with body 'Yo Pierre', got {:?}",
            msg.content
        );
    }

    #[test]
    fn parse_message_create_skips_empty_content() {
        let data = json!({
            "id": "123",
            "channel_id": "456",
            "content": "",
            "author": {
                "id": "789",
                "username": "User"
            }
        });

        assert!(parse_message_create(&data, None).is_none());
    }

    #[test]
    fn backoff_delay_increases_exponentially() {
        let d1 = calculate_backoff_delay(1);
        let d2 = calculate_backoff_delay(2);
        let d3 = calculate_backoff_delay(3);

        assert_eq!(d1, Duration::from_secs(2));
        assert_eq!(d2, Duration::from_secs(4));
        assert_eq!(d3, Duration::from_secs(8));
    }

    #[test]
    fn backoff_delay_caps_at_max() {
        let d = calculate_backoff_delay(20);
        assert_eq!(d, RECONNECT_MAX_DELAY);
    }
}
