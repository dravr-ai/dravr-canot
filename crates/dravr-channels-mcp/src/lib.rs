// ABOUTME: Library root re-exporting MCP server modules for use by dravr-channels-server
// ABOUTME: Exposes protocol, server, state, tools, and transport as public modules
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # dravr-channels-mcp
//!
//! MCP server library exposing dravr-channels messaging via Model Context Protocol.
//! Supports stdio and HTTP/SSE transports with channel management and message sending tools.
//!
//! ## Re-exports
//!
//! - [`McpServer`] — JSON-RPC request dispatcher
//! - [`ServerState`] / [`SharedState`] — shared server state with channel registry and configs
//! - [`build_tool_registry`] — default tool registry with all 4 MCP tools
//! - [`McpTransport`] — transport trait for stdio/HTTP backends

pub mod protocol;
pub mod server;
pub mod state;
pub mod tools;
pub mod transport;

pub use server::McpServer;
pub use state::{ServerState, SharedState};
pub use tools::build_tool_registry;
pub use transport::McpTransport;
