// ABOUTME: GET /health handler checking channel registration and readiness
// ABOUTME: Returns per-channel status and HTTP 200 if channels registered, 503 if none
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::state::SharedState;

/// Health check response listing registered channels and their status
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall server status: "ok" or "degraded"
    pub status: &'static str,
    /// Per-channel status map
    pub channels: HashMap<String, String>,
}

/// Handle GET /health
///
/// Lists all registered channels with their current status. Returns HTTP 200
/// if at least one channel is registered, 503 if the registry is empty.
pub async fn handle(State(state): State<SharedState>) -> impl IntoResponse {
    let registered = state.registry().registered_channels();

    let mut channels = HashMap::new();

    for channel_type in &registered {
        let status = if state.get_config(channel_type).await.is_some() {
            "configured"
        } else {
            "registered"
        };
        channels.insert(channel_type.to_string(), status.to_owned());
    }

    let any_registered = !channels.is_empty();
    let status_str = if any_registered { "ok" } else { "degraded" };
    let http_status = if any_registered {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let resp = HealthResponse {
        status: status_str,
        channels,
    };

    (http_status, Json(resp))
}
