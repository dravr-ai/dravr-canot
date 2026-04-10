// ABOUTME: Axum router wiring REST messaging and MCP endpoints
// ABOUTME: Mounts webhook, send, health, and MCP routes with optional auth middleware
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use dravr_tronc::mcp::transport::http::mcp_router as build_mcp_router;
use dravr_tronc::McpServer;

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
/// - `POST /mcp` — MCP Streamable HTTP (JSON-RPC 2.0, via dravr-tronc)
///
/// The auth middleware is applied to all routes. It only enforces
/// authentication when `DRAVR_CHANNELS_API_KEY` is set.
pub fn build(state: SharedState) -> Router {
    let mcp_server = Arc::new(McpServer::new(
        "dravr-canot",
        env!("CARGO_PKG_VERSION"),
        dravr_canot_mcp::build_tool_registry(),
        Arc::clone(&state),
    ));

    let mcp_router = build_mcp_router(mcp_server);

    Router::new()
        .route("/api/messaging/webhook/{channel}", post(webhook::handle))
        .route("/api/messaging/send", post(send::handle))
        .route("/health", get(health::handle))
        .with_state(state)
        .merge(mcp_router)
        .layer(middleware::from_fn(auth::require_auth))
}
