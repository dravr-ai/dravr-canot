// ABOUTME: Library root re-exporting server modules for integration testing
// ABOUTME: Enables tests/ to access router, state, types, and handler modules
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # dravr-channels-server
//!
//! Unified REST API + MCP server for multi-platform messaging channels.
//!
//! ## Endpoints
//!
//! - `POST /api/messaging/webhook/:channel` — receive inbound webhooks
//! - `POST /api/messaging/send` — send outbound messages
//! - `GET /health` — channel readiness check
//! - `POST /mcp` — MCP Streamable HTTP (JSON-RPC 2.0, via dravr-channels-mcp)
//!
//! ## Modules
//!
//! - [`webhook`] — inbound webhook handler with signature verification
//! - [`send`] — outbound message sending endpoint
//! - [`health`] — channel health checks
//! - [`auth`] — optional bearer token authentication
//! - [`router`] — Axum router wiring all endpoints (REST + MCP)
//! - [`state`] — re-export of unified state from dravr-channels-mcp

pub mod auth;
pub mod health;
pub mod router;
pub mod send;
pub mod state;
pub mod webhook;

pub use state::SharedState;
