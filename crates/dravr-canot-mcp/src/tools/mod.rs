// ABOUTME: Tool registry builder mapping MCP tool names to handler implementations
// ABOUTME: Delegates McpTool trait and ToolRegistry to dravr-tronc, registers canot-specific tools
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

pub mod channels;
pub mod config;
pub mod send;

use dravr_tronc::mcp::tool::ToolRegistry;

use crate::state::ServerState;

/// Build the default tool registry with all dravr-canot MCP tools
pub fn build_tool_registry() -> ToolRegistry<ServerState> {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(channels::ListChannels));
    registry.register(Box::new(send::SendMessage));
    registry.register(Box::new(config::GetChannelConfig));
    registry.register(Box::new(config::SetChannelConfig));
    registry
}
