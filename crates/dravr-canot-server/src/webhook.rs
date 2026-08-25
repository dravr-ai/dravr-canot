// ABOUTME: POST /api/messaging/webhook/:channel handler for inbound webhook processing
// ABOUTME: Verifies signatures, parses inbound messages, and returns them as JSON
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use tracing::{debug, warn};

use dravr_canot::error::ErrorResponse;
use dravr_canot::models::{ChannelType, InboundReaction, IncomingMessage};

use crate::state::SharedState;

/// Successful webhook response containing parsed inbound messages
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    /// Number of messages parsed from the webhook payload
    pub message_count: usize,
    /// Parsed inbound messages
    pub messages: Vec<IncomingMessage>,
    /// Number of reaction events parsed from the webhook payload
    pub reaction_count: usize,
    /// Parsed inbound reaction events. Empty for channels whose webhook
    /// API delivers no reaction events (see
    /// `ChannelDescriptor::delivers_inbound_reactions`).
    pub reactions: Vec<InboundReaction>,
}

/// Handle POST /api/messaging/webhook/:channel
///
/// Receives an inbound webhook from a messaging platform, verifies its
/// cryptographic signature using the registered channel adapter, parses
/// the payload into normalized `IncomingMessage` structs plus any
/// `InboundReaction` events, and returns them as JSON.
pub async fn handle(
    State(state): State<SharedState>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let channel_type: ChannelType = match channel.parse() {
        Ok(ct) => ct,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "invalid_channel",
                    format!("Unknown channel type: {channel}"),
                )),
            )
                .into_response();
        }
    };

    debug!(channel = %channel_type, "Received webhook");

    let registry = state.registry();

    let Some(adapter) = registry.get(&channel_type) else {
        warn!(channel = %channel_type, "No adapter registered for channel");
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(
                "channel_not_found",
                format!("No adapter registered for channel: {channel_type}"),
            )),
        )
            .into_response();
    };

    // Verify webhook signature
    if let Err(e) = adapter.verify_signature(&headers, &body) {
        warn!(channel = %channel_type, error = %e, "Webhook signature verification failed");
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse::new("signature_verification_failed", e)),
        )
            .into_response();
    }

    // Parse inbound messages
    let messages = match adapter.receive(&headers, &body).await {
        Ok(messages) => messages,
        Err(e) => {
            warn!(channel = %channel_type, error = %e, "Failed to parse webhook payload");
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("invalid_payload", e)),
            )
                .into_response();
        }
    };

    // Parse inbound reaction events from the same verified body. Channels
    // whose webhook API delivers no reaction events return an empty list.
    let reactions = match adapter.receive_reactions(&headers, &body).await {
        Ok(reactions) => reactions,
        Err(e) => {
            warn!(channel = %channel_type, error = %e, "Failed to parse webhook reactions");
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("invalid_payload", e)),
            )
                .into_response();
        }
    };

    debug!(
        channel = %channel_type,
        message_count = messages.len(),
        reaction_count = reactions.len(),
        "Parsed inbound webhook"
    );
    let resp = WebhookResponse {
        message_count: messages.len(),
        messages,
        reaction_count: reactions.len(),
        reactions,
    };
    (StatusCode::OK, Json(resp)).into_response()
}
