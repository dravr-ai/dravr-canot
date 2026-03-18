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
use dravr_canot::models::{ChannelType, IncomingMessage};

use crate::state::SharedState;

/// Successful webhook response containing parsed inbound messages
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    /// Number of messages parsed from the webhook payload
    pub message_count: usize,
    /// Parsed inbound messages
    pub messages: Vec<IncomingMessage>,
}

/// Handle POST /api/messaging/webhook/:channel
///
/// Receives an inbound webhook from a messaging platform, verifies its
/// cryptographic signature using the registered channel adapter, parses
/// the payload into normalized `IncomingMessage` structs, and returns
/// them as JSON.
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

    let state_guard = state.read().await;
    let registry = state_guard.registry();

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
    match adapter.receive(&headers, &body).await {
        Ok(messages) => {
            debug!(
                channel = %channel_type,
                count = messages.len(),
                "Parsed inbound messages"
            );
            let resp = WebhookResponse {
                message_count: messages.len(),
                messages,
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(e) => {
            warn!(channel = %channel_type, error = %e, "Failed to parse webhook payload");
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("invalid_payload", e)),
            )
                .into_response()
        }
    }
}
