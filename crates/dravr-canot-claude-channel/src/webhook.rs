// ABOUTME: Axum HTTP webhook server receiving inbound messages from Slack/Telegram
// ABOUTME: Verifies signatures, parses messages, gates by sender, and forwards to MCP via mpsc
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use dravr_canot::models::{ChannelType, MessageContent};
use dravr_canot::ChannelRegistry;

use crate::mcp::ChannelEvent;
use crate::sender_gate::SenderGate;

/// Shared state for the webhook HTTP server
pub struct WebhookState {
    /// Channel adapter registry for signature verification and message parsing
    pub registry: Arc<ChannelRegistry>,
    /// Sender allowlist for gating inbound messages
    pub sender_gate: Arc<SenderGate>,
    /// Channel for forwarding parsed messages to the MCP stdio loop
    pub event_tx: mpsc::Sender<ChannelEvent>,
}

/// Build the Axum router for the webhook server
pub fn build_router(state: Arc<WebhookState>) -> Router {
    Router::new()
        .route("/webhook/{channel}", post(handle_webhook))
        .with_state(state)
}

/// Handle POST /webhook/:channel
///
/// Receives an inbound webhook, verifies its signature, parses
/// messages, gates by sender, and forwards to the MCP loop.
/// Returns 200 immediately to acknowledge receipt (platforms
/// like Slack require a fast response).
async fn handle_webhook(
    State(state): State<Arc<WebhookState>>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, String) {
    let channel_type: ChannelType = match channel.parse() {
        Ok(ct) => ct,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Unknown channel type: {channel}"),
            );
        }
    };

    debug!(channel = %channel_type, "Received webhook");

    let Some(adapter) = state.registry.get(&channel_type) else {
        warn!(channel = %channel_type, "No adapter registered for channel");
        return (
            StatusCode::NOT_FOUND,
            format!("No adapter registered for channel: {channel_type}"),
        );
    };

    // Verify webhook signature
    if let Err(e) = adapter.verify_signature(&headers, &body) {
        warn!(channel = %channel_type, error = %e, "Signature verification failed");
        return (StatusCode::UNAUTHORIZED, format!("Signature failed: {e}"));
    }

    // Parse inbound messages
    let messages = match adapter.receive(&headers, &body).await {
        Ok(msgs) => msgs,
        Err(e) => {
            warn!(channel = %channel_type, error = %e, "Failed to parse webhook");
            return (StatusCode::BAD_REQUEST, format!("Parse error: {e}"));
        }
    };

    let mut forwarded = 0usize;
    for msg in &messages {
        // Gate on sender identity to prevent prompt injection
        if !state.sender_gate.is_allowed(&msg.sender_id) {
            debug!(
                sender_id = %msg.sender_id,
                channel = %channel_type,
                "Dropped message from non-allowlisted sender"
            );
            continue;
        }

        // Validate sender identity fields before propagation
        if msg.sender_id.is_empty() {
            debug!(
                channel = %channel_type,
                "Dropped message with empty sender_id"
            );
            continue;
        }

        let content_text = extract_text_content(&msg.content);
        let chat_id = msg
            .conversation_id
            .clone()
            .unwrap_or_else(|| msg.sender_id.clone());
        let sender_name = msg
            .sender_name
            .clone()
            .unwrap_or_else(|| msg.sender_id.clone());

        let event = ChannelEvent::Message {
            sender: sender_name,
            channel_type: channel_type.to_string(),
            chat_id,
            content: content_text,
        };

        match state.event_tx.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(channel = %channel_type, "Event channel full, dropping message");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Server overloaded".to_owned(),
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                warn!("Event channel closed, MCP loop may have exited");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal forwarding error".to_owned(),
                );
            }
        }

        forwarded += 1;
    }

    debug!(channel = %channel_type, count = forwarded, "Forwarded messages to MCP");
    (StatusCode::OK, "ok".to_owned())
}

/// Extract human-readable text from a `MessageContent` variant
fn extract_text_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text { body } => body.clone(),
        MessageContent::Media { caption, url, .. } => caption
            .as_deref()
            .map_or_else(|| format!("[media: {url}]"), String::from),
        MessageContent::Location {
            latitude,
            longitude,
        } => format!("[location: {latitude}, {longitude}]"),
        MessageContent::Card { title, body, .. } => format!("{title}: {body}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dravr_canot::models::ChannelType;
    use dravr_canot::ChannelRegistry;

    use super::*;
    use crate::sender_gate::SenderGate;
    use crate::test_support::{sample_incoming, MockAdapter};

    /// Build a `WebhookState` + receiver pair with the given adapter, allowlist, and buffer size.
    fn build_state(
        adapter: Arc<MockAdapter>,
        allowed_senders: &str,
        buffer: usize,
    ) -> (Arc<WebhookState>, mpsc::Receiver<ChannelEvent>) {
        let mut registry = ChannelRegistry::new();
        registry.register(adapter);
        let (event_tx, event_rx) = mpsc::channel(buffer);
        let state = Arc::new(WebhookState {
            registry: Arc::new(registry),
            sender_gate: Arc::new(SenderGate::from_csv(allowed_senders)),
            event_tx,
        });
        (state, event_rx)
    }

    async fn call_handler(state: Arc<WebhookState>, channel: &str) -> (StatusCode, String) {
        handle_webhook(
            State(state),
            Path(channel.to_owned()),
            HeaderMap::new(),
            Bytes::from_static(b"{}"),
        )
        .await
    }

    #[tokio::test]
    async fn returns_bad_request_for_unknown_channel_name() {
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack));
        let (state, _rx) = build_state(adapter, "", 16);
        let (status, body) = call_handler(state, "unknown").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("unknown"));
    }

    #[tokio::test]
    async fn returns_not_found_when_adapter_missing() {
        // Register a slack adapter but call /webhook/telegram
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack));
        let (state, _rx) = build_state(adapter, "", 16);
        let (status, _body) = call_handler(state, "telegram").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn returns_unauthorized_on_signature_failure() {
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_verify_failing());
        let (state, _rx) = build_state(adapter, "", 16);
        let (status, body) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("Signature failed"));
    }

    #[tokio::test]
    async fn returns_bad_request_on_parse_failure() {
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_parse_failing());
        let (state, _rx) = build_state(adapter, "", 16);
        let (status, body) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("Parse error"));
    }

    #[tokio::test]
    async fn forwards_valid_message_to_mpsc() {
        let msg = sample_incoming(ChannelType::Slack, "U123", Some("C456"), "hello");
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![msg]));
        let (state, mut rx) = build_state(adapter, "", 16);

        let (status, body) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");

        let event = rx.try_recv().expect("should have forwarded event"); // Safe: test assertion
        match event {
            ChannelEvent::Message {
                sender,
                channel_type,
                chat_id,
                content,
            } => {
                assert_eq!(sender, "U123-name");
                assert_eq!(channel_type, "slack");
                assert_eq!(chat_id, "C456");
                assert_eq!(content, "hello");
            }
        }
    }

    #[tokio::test]
    async fn drops_messages_from_non_allowlisted_senders() {
        let ok = sample_incoming(ChannelType::Slack, "U_ALLOWED", Some("C1"), "yes");
        let blocked = sample_incoming(ChannelType::Slack, "U_OTHER", Some("C1"), "spam");
        let adapter =
            Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![blocked, ok]));
        let (state, mut rx) = build_state(adapter, "U_ALLOWED", 16);

        let (status, _) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::OK);

        let event = rx.try_recv().expect("allowed message forwarded"); // Safe: test assertion
        let ChannelEvent::Message { sender, .. } = event;
        assert_eq!(sender, "U_ALLOWED-name");
        // Only one message should have made it through
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn drops_messages_with_empty_sender_id() {
        let mut msg = sample_incoming(ChannelType::Slack, "U1", Some("C1"), "hi");
        msg.sender_id = String::new();
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![msg]));
        let (state, mut rx) = build_state(adapter, "", 16);

        let (status, _) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::OK);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn uses_sender_id_when_no_conversation_id() {
        let msg = sample_incoming(ChannelType::Slack, "U1", None, "hi");
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![msg]));
        let (state, mut rx) = build_state(adapter, "", 16);

        let (status, _) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::OK);

        let ChannelEvent::Message { chat_id, .. } = rx.try_recv().expect("event forwarded"); // Safe: test assertion
        assert_eq!(chat_id, "U1");
    }

    #[tokio::test]
    async fn returns_service_unavailable_when_channel_full() {
        // Channel capacity 1 — fill it, then the webhook should get 503
        let msg1 = sample_incoming(ChannelType::Slack, "U1", Some("C1"), "one");
        let msg2 = sample_incoming(ChannelType::Slack, "U2", Some("C2"), "two");
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![msg1, msg2]));
        let (state, _rx) = build_state(adapter, "", 1);

        let (status, body) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("overloaded"));
    }

    #[tokio::test]
    async fn returns_internal_error_when_receiver_dropped() {
        let msg = sample_incoming(ChannelType::Slack, "U1", Some("C1"), "hi");
        let adapter = Arc::new(MockAdapter::new(ChannelType::Slack).with_inbound(vec![msg]));
        let (state, rx) = build_state(adapter, "", 16);
        drop(rx); // Simulate MCP loop having exited

        let (status, body) = call_handler(state, "slack").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains("forwarding"));
    }

    #[test]
    fn extract_text_from_text_content() {
        let content = MessageContent::Text {
            body: "hello".to_owned(),
        };
        assert_eq!(extract_text_content(&content), "hello");
    }

    #[test]
    fn extract_text_from_media_with_caption() {
        let content = MessageContent::Media {
            url: "https://example.com/img.jpg".to_owned(),
            mime_type: "image/jpeg".to_owned(),
            caption: Some("a photo".to_owned()),
        };
        assert_eq!(extract_text_content(&content), "a photo");
    }

    #[test]
    fn extract_text_from_media_without_caption() {
        let content = MessageContent::Media {
            url: "https://example.com/img.jpg".to_owned(),
            mime_type: "image/jpeg".to_owned(),
            caption: None,
        };
        assert_eq!(
            extract_text_content(&content),
            "[media: https://example.com/img.jpg]"
        );
    }

    #[test]
    fn extract_text_from_location() {
        let content = MessageContent::Location {
            latitude: 48.8566,
            longitude: 2.3522,
        };
        assert_eq!(
            extract_text_content(&content),
            "[location: 48.8566, 2.3522]"
        );
    }

    #[test]
    fn extract_text_from_card() {
        let content = MessageContent::Card {
            title: "Alert".to_owned(),
            body: "Something happened".to_owned(),
            actions: vec![],
        };
        assert_eq!(extract_text_content(&content), "Alert: Something happened");
    }
}
