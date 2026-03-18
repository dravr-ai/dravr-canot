// ABOUTME: Axum router wiring REST messaging and MCP endpoints
// ABOUTME: Mounts webhook, send, health, and MCP routes with optional auth middleware
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use crate::auth;
use crate::health;
use crate::send;
use crate::state::SharedState;
use crate::webhook;

/// Build the application router with all endpoints
///
/// Routes:
/// - `POST /api/messaging/webhook/:channel` — Inbound webhook processing
/// - `POST /api/messaging/send` — Outbound message delivery
/// - `GET /health` — Channel readiness check
/// - `POST /mcp` — MCP Streamable HTTP (JSON-RPC 2.0, via dravr-canot-mcp)
///
/// The auth middleware is applied to all routes. It only enforces
/// authentication when `DRAVR_CHANNELS_API_KEY` is set.
pub fn build(state: SharedState) -> Router {
    let mcp_server = Arc::new(dravr_canot_mcp::McpServer::new(
        Arc::clone(&state),
        dravr_canot_mcp::build_tool_registry(),
    ));

    let mcp_router = Router::new()
        .route(
            "/mcp",
            post(dravr_canot_mcp::transport::http::handle_mcp_post),
        )
        .with_state(mcp_server);

    Router::new()
        .route("/api/messaging/webhook/{channel}", post(webhook::handle))
        .route("/api/messaging/send", post(send::handle))
        .route("/health", get(health::handle))
        .with_state(state)
        .merge(mcp_router)
        .layer(middleware::from_fn(auth::require_auth))
}
