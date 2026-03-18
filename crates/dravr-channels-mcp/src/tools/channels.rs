// ABOUTME: MCP tool for listing registered messaging channels and their capabilities
// ABOUTME: Returns channel descriptor metadata including name, webhook path, and media support
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::protocol::{CallToolResult, ToolDefinition};
use crate::state::SharedState;
use crate::tools::McpTool;

/// Lists all registered messaging channels with their descriptor metadata
pub struct ListChannels;

#[async_trait]
impl McpTool for ListChannels {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_channels".to_owned(),
            description: "List all registered messaging channels and their capabilities".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(&self, state: &SharedState, _arguments: Value) -> CallToolResult {
        let state = state.read().await;
        let registry = state.registry();
        let channel_types = registry.registered_channels();

        let channels: Vec<Value> = channel_types
            .iter()
            .filter_map(|ct| {
                registry.get(ct).map(|channel| {
                    json!({
                        "channel_type": ct.to_string(),
                        "name": channel.channel_type().to_string(),
                        "supports_media": false,
                        "registered": true,
                    })
                })
            })
            .collect();

        CallToolResult::text(
            json!({
                "channel_count": channels.len(),
                "channels": channels,
            })
            .to_string(),
        )
    }
}
