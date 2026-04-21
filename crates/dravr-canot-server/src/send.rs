// ABOUTME: POST /api/messaging/send handler for outbound message delivery
// ABOUTME: Accepts JSON body with channel, recipient, and content, returns delivery receipt
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tracing::{debug, warn};

use dravr_canot::error::ErrorResponse;
use dravr_canot::models::{ChannelConfig, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::turn::ConversationTurnId;

use crate::state::SharedState;

/// Request body for the send message endpoint
#[derive(Debug, Deserialize)]
pub struct SendRequest {
    /// Target channel platform (e.g., "whatsapp", "slack")
    pub channel_type: ChannelType,
    /// Platform-specific recipient identifier
    pub recipient_id: String,
    /// Message content to send
    pub content: MessageContent,
    /// Conversation-turn correlation identifier carried from the inbound
    /// boundary. Callers must propagate the identifier they received;
    /// this endpoint never generates one.
    pub turn_id: ConversationTurnId,
    /// Optional channel configuration override (uses server config if omitted)
    #[serde(default)]
    pub config: Option<ChannelConfig>,
}

/// Handle POST /api/messaging/send
///
/// Sends an outbound message through the specified channel adapter.
/// Uses channel configuration from the request body if provided,
/// falling back to the server's stored configuration for that channel.
pub async fn handle(
    State(state): State<SharedState>,
    Json(request): Json<SendRequest>,
) -> impl IntoResponse {
    debug!(
        channel = %request.channel_type,
        recipient = %request.recipient_id,
        "Sending outbound message"
    );

    let state_guard = state.read().await;
    let registry = state_guard.registry();

    let Some(adapter) = registry.get(&request.channel_type) else {
        warn!(channel = %request.channel_type, "No adapter registered for channel");
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(
                "channel_not_found",
                format!(
                    "No adapter registered for channel: {}",
                    request.channel_type
                ),
            )),
        )
            .into_response();
    };

    // Resolve channel config: request override takes precedence over server state
    let config = if let Some(cfg) = request.config {
        cfg
    } else if let Some(cfg) = state_guard.get_config(&request.channel_type) {
        cfg.clone()
    } else {
        warn!(channel = %request.channel_type, "No configuration for channel");
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse::new(
                "channel_not_configured",
                format!(
                    "No configuration available for channel: {}",
                    request.channel_type
                ),
            )),
        )
            .into_response();
    };

    let outgoing = OutgoingMessage {
        channel_type: request.channel_type,
        recipient_id: request.recipient_id,
        content: request.content,
        turn_id: request.turn_id,
        reply_to: None,
        thread_id: None,
    };

    match adapter.send(&outgoing, &config).await {
        Ok(receipt) => {
            debug!(
                channel = %request.channel_type,
                message_id = %receipt.message_id,
                status = ?receipt.status,
                "Message sent"
            );
            (StatusCode::OK, Json(receipt)).into_response()
        }
        Err(e) => {
            warn!(
                channel = %request.channel_type,
                error = %e,
                "Message delivery failed"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse::new("delivery_failed", e)),
            )
                .into_response()
        }
    }
}
