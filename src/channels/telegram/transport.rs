// ABOUTME: Telegram Bot API transport adapter for webhook parsing and message sending
// ABOUTME: Secret token header verification with constant-time comparison, Update payload parsing
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use crate::error::{MessagingError, MessagingResult};
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, DeliveryStatus, InboundReaction, IncomingMessage,
    MessageContent, ReactionAction,
};
use async_trait::async_trait;
use chrono::Utc;
use http::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::http_client::api_client;

use crate::transport::TransportAdapter;
use crate::turn::ConversationTurnId;

/// Process-wide cache of resolved bot usernames, keyed by bot id.
///
/// Telegram exposes the bot's username only via `getMe`; the username is
/// stable for the lifetime of a bot token, so one successful resolution per
/// process serves every subsequent webhook. Transports are constructed
/// per-request, which is why the cache is a static rather than a field.
static BOT_USERNAMES: LazyLock<Mutex<HashMap<i64, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Telegram Bot API transport adapter
///
/// Verification: `X-Telegram-Bot-Api-Secret-Token` header matched against
/// the configured webhook secret using constant-time comparison.
pub struct TelegramTransport {
    /// Shared HTTP client for outbound Bot API calls
    client: &'static reqwest::Client,
    /// Expected secret token for webhook verification
    webhook_secret: String,
    /// Numeric bot user id, derived from the `bot_token` prefix (the part
    /// before `:`). `None` when the transport was built without a token,
    /// which disables bot-addressing detection (every group message parses
    /// with `addressed_to_bot: false`).
    bot_id: Option<i64>,
    /// Bot API token used to lazily resolve the bot's username via `getMe`
    /// for @-mention matching. `None` disables username resolution.
    bot_token: Option<String>,
}

impl TelegramTransport {
    /// Create a transport with the given webhook secret and no bot identity
    /// (bot-addressing detection disabled).
    #[must_use]
    pub fn new(webhook_secret: String) -> Self {
        Self {
            client: api_client(),
            webhook_secret,
            bot_id: None,
            bot_token: None,
        }
    }

    /// Create a transport that can detect bot-addressed messages. The bot id
    /// is derived from the token's numeric prefix; the username is resolved
    /// lazily via `getMe` and cached process-wide.
    #[must_use]
    pub fn with_bot_token(webhook_secret: String, bot_token: Option<String>) -> Self {
        Self {
            client: api_client(),
            webhook_secret,
            bot_id: bot_token.as_deref().and_then(bot_id_from_token),
            bot_token,
        }
    }

    /// Create a transport with a fully known bot identity, seeding the
    /// username cache so no `getMe` call is ever made. Used by tests and by
    /// callers that already resolved the identity.
    #[must_use]
    pub fn with_bot_identity(webhook_secret: String, bot_id: i64, bot_username: &str) -> Self {
        if let Ok(mut cache) = BOT_USERNAMES.lock() {
            cache.insert(bot_id, bot_username.to_owned());
        }
        Self {
            client: api_client(),
            webhook_secret,
            bot_id: Some(bot_id),
            bot_token: None,
        }
    }

    /// Resolve the bot's username, from the process-wide cache when warm,
    /// otherwise via one `getMe` call. Returns `None` (and logs at debug)
    /// when no token is configured or the call fails — mention detection
    /// Acknowledge a tapped inline-keyboard button.
    ///
    /// Telegram spins a progress indicator on the button until the bot calls
    /// `answerCallbackQuery`, and if the call never comes the user is shown
    /// nothing — a tap that was processed perfectly still reads as broken. The
    /// coach picker surfaced this: selections landed while the athlete saw no
    /// response.
    ///
    /// Best-effort by design. The callback has already been parsed into a
    /// message by the time this runs, so a failed acknowledgement costs a
    /// spinner, never the selection.
    async fn answer_callback_query(&self, callback: &Value) {
        let Some(bot_token) = self.bot_token.as_deref() else {
            debug!("telegram callback not acknowledged: no bot token configured");
            return;
        };
        let Some(callback_id) = callback.get("id").and_then(Value::as_str) else {
            debug!("telegram callback carries no id; nothing to acknowledge");
            return;
        };

        let url = format!("https://api.telegram.org/bot{bot_token}/answerCallbackQuery");
        match self
            .client
            .post(&url)
            .json(&serde_json::json!({ "callback_query_id": callback_id }))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                debug!("telegram callback acknowledged");
            }
            Ok(response) => {
                warn!(status = %response.status(), "telegram answerCallbackQuery rejected");
            }
            Err(e) => {
                warn!(error = %e, "telegram answerCallbackQuery failed; the button will spin");
            }
        }
    }

    /// then degrades to reply/`text_mention` signals for this message.
    async fn bot_username(&self) -> Option<String> {
        let bot_id = self.bot_id?;
        if let Some(cached) = BOT_USERNAMES
            .lock()
            .ok()
            .and_then(|cache| cache.get(&bot_id).cloned())
        {
            return Some(cached);
        }
        let bot_token = self.bot_token.as_deref()?;
        let url = format!("https://api.telegram.org/bot{bot_token}/getMe");
        let response = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "telegram getMe failed; @-mention detection degraded for this message");
                return None;
            }
        };
        let body: Value = response.json().await.ok()?;
        let username = body
            .pointer("/result/username")
            .and_then(Value::as_str)
            .map(str::to_owned)?;
        if let Ok(mut cache) = BOT_USERNAMES.lock() {
            cache.insert(bot_id, username.clone());
        }
        info!(bot_id, "telegram bot username resolved via getMe");
        Some(username)
    }

    /// `true` when a group message explicitly addresses this bot: a reply to
    /// one of the bot's own messages, a `text_mention` entity naming the bot
    /// user, or an `@username` mention in the text or media caption.
    async fn message_addresses_bot(&self, message: &Value) -> bool {
        let Some(bot_id) = self.bot_id else {
            return false;
        };
        if message
            .pointer("/reply_to_message/from/id")
            .and_then(Value::as_i64)
            == Some(bot_id)
        {
            return true;
        }
        // `text_mention` entities (mentions of users without a username)
        // carry the mentioned user object directly — no text slicing needed.
        for entities_key in ["entities", "caption_entities"] {
            if let Some(entities) = message.get(entities_key).and_then(Value::as_array) {
                if entities.iter().any(|e| {
                    e.get("type").and_then(Value::as_str) == Some("text_mention")
                        && e.pointer("/user/id").and_then(Value::as_i64) == Some(bot_id)
                }) {
                    return true;
                }
            }
        }
        // `@username` mentions (and `/command@username` targeting) appear
        // verbatim in the text/caption, so a boundary-checked search avoids
        // decoding entity offsets (which Telegram counts in UTF-16 units).
        let Some(username) = self.bot_username().await else {
            return false;
        };
        ["text", "caption"].iter().any(|key| {
            message
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|text| text_mentions_username(text, &username))
        })
    }
}

/// Derive the numeric bot id from a Bot API token (`<bot_id>:<secret>`).
fn bot_id_from_token(token: &str) -> Option<i64> {
    token.split(':').next()?.parse().ok()
}

/// Case-insensitive search for `@username` in `text`, requiring a word
/// boundary after the match so `@mybot` does not fire inside `@mybotfan`.
/// Telegram usernames are ASCII (`[A-Za-z0-9_]{5,32}`), so ASCII-lowercase
/// comparison is sufficient.
fn text_mentions_username(text: &str, username: &str) -> bool {
    let needle = format!("@{}", username.to_ascii_lowercase());
    let haystack = text.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(pos) = haystack[search_from..].find(&needle) {
        let end = search_from + pos + needle.len();
        let boundary = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        if boundary {
            return true;
        }
        search_from = end;
    }
    false
}

#[async_trait]
impl TransportAdapter for TelegramTransport {
    fn verify_signature(&self, headers: &HeaderMap, _body: &[u8]) -> MessagingResult<()> {
        let secret_header = headers
            .get("x-telegram-bot-api-secret-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| MessagingError::SignatureVerificationFailed {
                channel: "telegram".to_owned(),
                reason: "missing X-Telegram-Bot-Api-Secret-Token header".to_owned(),
            })?;

        // Constant-time comparison to prevent timing attacks
        let expected = self.webhook_secret.as_bytes();
        let received = secret_header.as_bytes();
        let equal: bool = subtle::ConstantTimeEq::ct_eq(received, expected).into();

        if equal {
            Ok(())
        } else {
            Err(MessagingError::SignatureVerificationFailed {
                channel: "telegram".to_owned(),
                reason: "secret token mismatch".to_owned(),
            })
        }
    }

    async fn parse_inbound(
        &self,
        _headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<IncomingMessage>> {
        // Webhook payload arrives as raw JSON. Operator-facing summary at
        // info; full body at trace so an operator following a typed message
        // through the stack can see exactly what Telegram delivered without
        // inspecting their reverse proxy.
        info!(body_len = body.len(), "telegram parse_inbound received");
        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                body = %String::from_utf8_lossy(body),
                "telegram inbound body"
            );
        }

        let update: Value =
            serde_json::from_slice(body).map_err(|e| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: format!("invalid JSON: {e}"),
            })?;

        // Telegram sends one Update per webhook — check for callback_query first (button taps)
        if let Some(callback) = update.get("callback_query") {
            // Acknowledge before returning. Telegram keeps a progress indicator
            // spinning on the tapped button until the bot answers, and shows the
            // user nothing at all if it never does — so a tap that IS processed
            // still reads as a dead button. That was the reported symptom for the
            // coach picker: the selection landed server-side while the athlete saw
            // no response and reasonably concluded it was broken.
            //
            // Deliberately best-effort and non-blocking on failure: the message it
            // acknowledges has already been parsed, and losing the acknowledgement
            // must not lose the selection.
            self.answer_callback_query(callback).await;
            return Ok(parse_callback_query(callback, &update));
        }

        let Some(message) = update.get("message") else {
            debug!("Telegram update without message or callback_query field");
            return Ok(vec![]);
        };

        // Skip Telegram service messages (member added/removed, group created, etc.)
        // These are not user-authored content and should not be dispatched to the LLM.
        if is_service_message(message) {
            debug!("Skipping Telegram service message");
            return Ok(vec![]);
        }

        let chat_id = message
            .pointer("/chat/id")
            .and_then(Value::as_i64)
            .ok_or_else(|| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: "missing chat.id".to_owned(),
            })?;

        // Telegram chat.type is one of "private", "group", "supergroup",
        // "channel" (Bot API getUpdates spec). Only "private" is a 1:1 DM.
        let is_direct_message =
            message.pointer("/chat/type").and_then(Value::as_str) == Some("private");

        // Telegram exposes chat.title for group / supergroup / channel
        // (Bot API). Private chats carry first_name/last_name instead and
        // produce None here.
        let chat_title = message
            .pointer("/chat/title")
            .and_then(Value::as_str)
            .map(str::to_owned);

        let from_id = message
            .pointer("/from/id")
            .and_then(Value::as_i64)
            .unwrap_or(chat_id);

        let from_name = message
            .pointer("/from/first_name")
            .and_then(Value::as_str)
            .map(str::to_owned);

        let message_id = message
            .get("message_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let content = message.get("text").and_then(Value::as_str).map_or_else(
            || parse_non_text_content(message),
            |text| MessageContent::Text {
                body: text.to_owned(),
            },
        );

        // A DM is inherently addressed to the bot; a group message counts
        // only when it mentions the bot or replies to one of its messages.
        let addressed_to_bot = is_direct_message || self.message_addresses_bot(message).await;

        // Extract forum topic thread ID for groups with Topics enabled
        let metadata = message
            .get("message_thread_id")
            .and_then(Value::as_i64)
            .map_or(
                Value::Null,
                |thread_id| serde_json::json!({ "message_thread_id": thread_id }),
            );

        let incoming = IncomingMessage {
            channel_type: ChannelType::Telegram,
            sender_id: from_id.to_string(),
            sender_name: from_name,
            content,
            conversation_id: Some(chat_id.to_string()),
            chat_title,
            channel_message_id: message_id.to_string(),
            timestamp: Utc::now(),
            raw_payload: update,
            turn_id: ConversationTurnId::new(),
            is_direct_message,
            addressed_to_bot,
            metadata,
        };

        info!(
            turn_id = %incoming.turn_id,
            chat_id,
            message_id,
            is_direct_message,
            addressed_to_bot,
            content_kind = match &incoming.content {
                MessageContent::Text { .. } => "text",
                _ => "non_text",
            },
            "telegram inbound message parsed"
        );

        Ok(vec![incoming])
    }

    /// Parse a Telegram `message_reaction` update into reaction events.
    ///
    /// Telegram reports the reactor's full reaction set before and after
    /// the change (`old_reaction` / `new_reaction`), so one update can
    /// yield several events — e.g. swapping 👍 for ❤ emits one `Removed`
    /// and one `Added`. Delivered only when the bot's webhook is
    /// registered with `allowed_updates` including `"message_reaction"`
    /// (the Bot API excludes it from the default update set).
    async fn parse_reactions(
        &self,
        _headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<InboundReaction>> {
        let update: Value =
            serde_json::from_slice(body).map_err(|e| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: format!("invalid JSON: {e}"),
            })?;

        let Some(reaction) = update.get("message_reaction") else {
            debug!("Telegram update without message_reaction field; no reactions parsed");
            return Ok(vec![]);
        };

        parse_message_reaction(reaction, &update)
    }

    async fn send_raw(
        &self,
        payload: &Value,
        turn_id: ConversationTurnId,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        let bot_token =
            config
                .bot_token
                .as_deref()
                .ok_or_else(|| MessagingError::ChannelNotConfigured {
                    channel: "telegram".to_owned(),
                })?;

        let method = resolve_bot_api_method(payload);
        let url = format!("https://api.telegram.org/bot{bot_token}/{method}");

        let response = self
            .client
            .post(&url)
            .json(payload)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "telegram".to_owned(),
                reason: format!("HTTP request failed: {e}"),
                retryable: true,
            })?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_owned());
            return Err(MessagingError::ChannelApiError {
                channel: "telegram".to_owned(),
                status_code: status,
                message: body_text,
            });
        }

        let result: Value = response
            .json()
            .await
            .map_err(|e| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: format!("invalid response JSON: {e}"),
            })?;

        let channel_message_id = result
            .pointer("/result/message_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string());

        Ok(DeliveryReceipt {
            message_id: Uuid::new_v4().to_string(),
            channel_message_id,
            status: DeliveryStatus::Sent,
            timestamp: Utc::now(),
            turn_id,
        })
    }
}

impl TelegramTransport {
    /// Delete a message from a chat via the Telegram `deleteMessage` Bot API.
    ///
    /// `chat_id` is the room/chat the message lives in and `message_id` is
    /// the Telegram message id to remove. Telegram requires `message_id` to
    /// be an integer and only honours the call when the bot is an admin with
    /// `can_delete_messages` in the chat; otherwise it returns HTTP 400 and
    /// this surfaces a [`MessagingError::ChannelApiError`].
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::ChannelNotConfigured`] when no bot token is
    /// present, [`MessagingError::DeliveryFailed`] when `message_id` is not a
    /// valid integer or the request can't be sent, and
    /// [`MessagingError::ChannelApiError`] when Telegram rejects the deletion.
    pub async fn delete_message(
        &self,
        chat_id: &str,
        message_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<()> {
        let bot_token =
            config
                .bot_token
                .as_deref()
                .ok_or_else(|| MessagingError::ChannelNotConfigured {
                    channel: "telegram".to_owned(),
                })?;

        let payload = build_delete_message_payload(chat_id, message_id)?;

        let url = format!("https://api.telegram.org/bot{bot_token}/deleteMessage");
        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "telegram".to_owned(),
                reason: format!("HTTP request failed: {e}"),
                retryable: true,
            })?;

        if response.status().is_success() {
            debug!(chat_id, message_id, "telegram message deleted");
            return Ok(());
        }

        let status = response.status().as_u16();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown".to_owned());
        Err(MessagingError::ChannelApiError {
            channel: "telegram".to_owned(),
            status_code: status,
            message: body_text,
        })
    }
}

/// Coerce a Telegram chat id string into the Bot API's "Integer or String"
/// shape: numeric ids (including negative supergroup ids) become a JSON
/// number, everything else (e.g. an `@channelusername`) stays a string.
fn chat_id_to_value(chat_id: &str) -> Value {
    chat_id
        .parse::<i64>()
        .map_or_else(|_| Value::from(chat_id), Value::from)
}

/// Build the `deleteMessage` request body.
///
/// Telegram requires `message_id` to be an integer, so the platform's string
/// id is parsed here; `chat_id` follows the Bot API "Integer or String" rule
/// (see [`chat_id_to_value`]).
///
/// # Errors
///
/// Returns [`MessagingError::DeliveryFailed`] when `message_id` is not a valid
/// integer.
fn build_delete_message_payload(chat_id: &str, message_id: &str) -> MessagingResult<Value> {
    let message_id_int: i64 = message_id
        .parse()
        .map_err(|e| MessagingError::DeliveryFailed {
            channel: "telegram".to_owned(),
            reason: format!("invalid message_id '{message_id}': {e}"),
            retryable: false,
        })?;

    Ok(serde_json::json!({
        "chat_id": chat_id_to_value(chat_id),
        "message_id": message_id_int,
    }))
}

/// Resolve the Telegram Bot API method from the rendered payload shape
///
/// The renderer produces payloads with different keys depending on content type:
/// - `"photo"` key → `sendPhoto`
/// - `"latitude"` key → `sendLocation`
/// - default → `sendMessage` (text and card content)
fn resolve_bot_api_method(payload: &Value) -> &'static str {
    if payload.get("photo").is_some() {
        "sendPhoto"
    } else if payload.get("latitude").is_some() {
        "sendLocation"
    } else {
        "sendMessage"
    }
}

/// Parse a Telegram `MessageReactionUpdated` object into reaction events,
/// diffing `old_reaction` against `new_reaction` per reaction name.
///
/// The reactor is `user` for a named user, or `actor_chat` when the chat
/// reacted anonymously (Telegram sends exactly one of the two).
///
/// # Errors
///
/// Returns [`MessagingError::InvalidPayload`] when `chat.id`, `message_id`,
/// or the reactor (`user.id` / `actor_chat.id`) is missing.
fn parse_message_reaction(
    reaction: &Value,
    update: &Value,
) -> MessagingResult<Vec<InboundReaction>> {
    let missing = |field: &str| MessagingError::InvalidPayload {
        channel: "telegram".to_owned(),
        reason: format!("message_reaction missing {field}"),
    };

    let chat_id = reaction
        .pointer("/chat/id")
        .and_then(Value::as_i64)
        .ok_or_else(|| missing("chat.id"))?;
    let message_id = reaction
        .get("message_id")
        .and_then(Value::as_i64)
        .ok_or_else(|| missing("message_id"))?;
    let reactor_id = reaction
        .pointer("/user/id")
        .and_then(Value::as_i64)
        .or_else(|| reaction.pointer("/actor_chat/id").and_then(Value::as_i64))
        .ok_or_else(|| missing("user.id / actor_chat.id"))?;

    let old_names = reaction_names(reaction.get("old_reaction"));
    let new_names = reaction_names(reaction.get("new_reaction"));

    let build = |emoji: &str, action: ReactionAction| InboundReaction {
        channel_type: ChannelType::Telegram,
        channel_message_id: message_id.to_string(),
        reactor_id: reactor_id.to_string(),
        emoji: emoji.to_owned(),
        action,
        conversation_id: Some(chat_id.to_string()),
        timestamp: Utc::now(),
        raw_payload: update.clone(),
    };

    let mut events: Vec<InboundReaction> = new_names
        .iter()
        .filter(|name| !old_names.contains(name))
        .map(|name| build(name, ReactionAction::Added))
        .collect();
    events.extend(
        old_names
            .iter()
            .filter(|name| !new_names.contains(name))
            .map(|name| build(name, ReactionAction::Removed)),
    );

    for event in &events {
        info!(
            chat_id,
            message_id,
            emoji = %event.emoji,
            action = %event.action,
            "telegram inbound reaction parsed"
        );
    }

    Ok(events)
}

/// Extract reaction names from a Telegram `ReactionType` array
/// (`old_reaction` / `new_reaction`).
fn reaction_names(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(reaction_name).collect())
        .unwrap_or_default()
}

/// Name a single Telegram `ReactionType` object: the emoji itself for
/// `emoji`, the custom-emoji id for `custom_emoji`, and the literal type
/// tag for tagged variants that carry no name of their own (e.g. `paid`).
fn reaction_name(reaction: &Value) -> Option<String> {
    match reaction.get("type").and_then(Value::as_str)? {
        "emoji" => reaction
            .get("emoji")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "custom_emoji" => reaction
            .get("custom_emoji_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        other => Some(other.to_owned()),
    }
}

/// Parse a Telegram `callback_query` update (inline keyboard button tap)
///
/// The `callback_query` contains the button's `data` field and the original message context.
fn parse_callback_query(callback: &Value, update: &Value) -> Vec<IncomingMessage> {
    let from_id = callback
        .pointer("/from/id")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let from_name = callback
        .pointer("/from/first_name")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let chat_id = callback
        .pointer("/message/chat/id")
        .and_then(Value::as_i64)
        .unwrap_or(from_id);

    // Mirror the message-path rule: callback_query messages carry the same
    // chat.type shape inside /message/chat/type.
    let is_direct_message = callback
        .pointer("/message/chat/type")
        .and_then(Value::as_str)
        == Some("private");

    let chat_title = callback
        .pointer("/message/chat/title")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let callback_data = callback.get("data").and_then(Value::as_str).unwrap_or("");
    let callback_id = callback.get("id").and_then(Value::as_str).unwrap_or("0");

    // Extract forum topic thread ID from the original message context
    let metadata = callback
        .pointer("/message/message_thread_id")
        .and_then(Value::as_i64)
        .map_or(
            Value::Null,
            |thread_id| serde_json::json!({ "message_thread_id": thread_id }),
        );

    let incoming = IncomingMessage {
        channel_type: ChannelType::Telegram,
        sender_id: from_id.to_string(),
        sender_name: from_name,
        content: MessageContent::Text {
            body: callback_data.to_owned(),
        },
        conversation_id: Some(chat_id.to_string()),
        chat_title,
        channel_message_id: callback_id.to_owned(),
        timestamp: Utc::now(),
        raw_payload: update.clone(),
        turn_id: ConversationTurnId::new(),
        is_direct_message,
        // Tapping the bot's own inline keyboard is an explicit interaction
        // with the bot regardless of the chat kind.
        addressed_to_bot: true,
        metadata,
    };

    vec![incoming]
}

/// Telegram service messages that are not user-authored content
///
/// These updates carry metadata about group changes (member joins/leaves,
/// title/photo changes, migrations) and should be silently ignored rather
/// than dispatched to the LLM pipeline.
const SERVICE_MESSAGE_FIELDS: &[&str] = &[
    "new_chat_members",
    "left_chat_member",
    "new_chat_title",
    "new_chat_photo",
    "delete_chat_photo",
    "group_chat_created",
    "supergroup_chat_created",
    "channel_chat_created",
    "pinned_message",
    "migrate_to_chat_id",
    "migrate_from_chat_id",
    "message_auto_delete_timer_changed",
    "forum_topic_created",
    "forum_topic_closed",
    "forum_topic_reopened",
    "video_chat_started",
    "video_chat_ended",
    "video_chat_participants_invited",
];

/// Check if a Telegram message is a service notification rather than user content
fn is_service_message(message: &Value) -> bool {
    SERVICE_MESSAGE_FIELDS
        .iter()
        .any(|field| message.get(field).is_some())
}

/// Parse non-text message content (location, photo, or unsupported)
fn parse_non_text_content(message: &Value) -> MessageContent {
    if let Some(location) = message.get("location") {
        let lat = location
            .get("latitude")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let lon = location
            .get("longitude")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        return MessageContent::Location {
            latitude: lat,
            longitude: lon,
        };
    }

    if message.get("photo").is_some() {
        // Photo array — take the largest (last) entry
        let photo_url = message
            .get("photo")
            .and_then(Value::as_array)
            .and_then(|arr| arr.last())
            .and_then(|p| p.get("file_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let caption = message
            .get("caption")
            .and_then(Value::as_str)
            .map(str::to_owned);
        return MessageContent::Media {
            url: photo_url,
            mime_type: "image/jpeg".to_owned(),
            caption,
        };
    }

    warn!("Telegram message with unsupported content type");
    MessageContent::Text {
        body: "[unsupported message type]".to_owned(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::{build_delete_message_payload, chat_id_to_value};
    use serde_json::Value;

    #[test]
    fn delete_payload_uses_integer_message_id_and_numeric_chat() {
        let payload = build_delete_message_payload("-1001234567890", "42").unwrap_or(Value::Null);
        // chat_id stays numeric, message_id is an integer (not a string).
        assert_eq!(payload["chat_id"], Value::from(-1_001_234_567_890_i64));
        assert_eq!(payload["message_id"], Value::from(42_i64));
    }

    #[test]
    fn delete_payload_keeps_username_chat_as_string() {
        let payload = build_delete_message_payload("@dravrchannel", "7").unwrap_or(Value::Null);
        assert_eq!(payload["chat_id"], Value::from("@dravrchannel"));
        assert_eq!(payload["message_id"], Value::from(7_i64));
    }

    #[test]
    fn delete_payload_rejects_non_integer_message_id() {
        // A non-numeric message id can't be sent to deleteMessage.
        assert!(build_delete_message_payload("99", "not-a-number").is_err());
    }

    #[test]
    fn numeric_chat_id_becomes_a_json_number() {
        // Private/user chats use small positive ids.
        assert_eq!(chat_id_to_value("99"), Value::from(99_i64));
    }

    #[test]
    fn negative_supergroup_id_stays_a_json_number() {
        // Supergroup chat ids are large negative integers; they must remain
        // numbers so Telegram routes deleteMessage to the right chat.
        assert_eq!(
            chat_id_to_value("-1001234567890"),
            Value::from(-1_001_234_567_890_i64)
        );
    }

    #[test]
    fn username_chat_id_stays_a_string() {
        // The @channelusername form can't parse as an integer and is sent
        // verbatim as a string.
        assert_eq!(
            chat_id_to_value("@dravrchannel"),
            Value::from("@dravrchannel")
        );
    }

    #[test]
    fn bot_id_derives_from_token_prefix() {
        assert_eq!(
            super::bot_id_from_token("123456789:AAFakeSecretPart"),
            Some(123_456_789)
        );
        assert_eq!(super::bot_id_from_token("not-a-token"), None);
        assert_eq!(super::bot_id_from_token(""), None);
    }

    #[test]
    fn username_mention_requires_word_boundary() {
        use super::text_mentions_username;
        assert!(text_mentions_username("@dravr_bot hello", "dravr_bot"));
        assert!(text_mentions_username("hey @Dravr_Bot!", "dravr_bot"));
        assert!(text_mentions_username("ping @dravr_bot", "dravr_bot"));
        assert!(text_mentions_username("/status@dravr_bot", "dravr_bot"));
        // A longer username containing ours must not match…
        assert!(!text_mentions_username("cc @dravr_botfan", "dravr_bot"));
        // …but a real mention after a false prefix still must.
        assert!(text_mentions_username(
            "cc @dravr_botfan and @dravr_bot",
            "dravr_bot"
        ));
        assert!(!text_mentions_username("no mention here", "dravr_bot"));
    }
}
