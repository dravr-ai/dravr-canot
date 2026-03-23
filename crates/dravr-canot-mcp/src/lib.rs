// ABOUTME: Library root re-exporting MCP server modules for use by dravr-canot-server
// ABOUTME: Delegates protocol, server, and transport to dravr-tronc; provides tools and state
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # dravr-canot-mcp
//!
//! MCP server library exposing dravr-canot messaging via Model Context Protocol.
//! Supports stdio and HTTP/SSE transports with channel management and message sending tools.
//!
//! ## Re-exports
//!
//! - [`McpServer`] — JSON-RPC request dispatcher (from dravr-tronc)
//! - [`ServerState`] / [`SharedState`] — shared server state with channel registry and configs
//! - [`build_tool_registry`] — default tool registry with all 4 MCP tools

pub mod state;
pub mod tools;

pub use dravr_tronc::McpServer;
pub use state::{ServerState, SharedState};
pub use tools::build_tool_registry;
