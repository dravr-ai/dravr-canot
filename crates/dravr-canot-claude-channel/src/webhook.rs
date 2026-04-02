// ABOUTME: Axum HTTP webhook server receiving inbound messages from Slack/Telegram
// ABOUTME: Verifies signatures, parses messages, gates by sender, and forwards to MCP via mpsc
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
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
) -> impl IntoResponse {
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
            sender_id: msg.sender_id.clone(),
            channel_type: channel_type.to_string(),
            chat_id,
            content: content_text,
        };

        if let Err(e) = state.event_tx.send(event).await {
            warn!(error = %e, "Failed to forward message to MCP loop");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal forwarding error".to_owned(),
            );
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
    use super::*;

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
