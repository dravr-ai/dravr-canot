// ABOUTME: MCP tool for listing registered messaging channels and their capabilities
// ABOUTME: Returns real descriptor metadata - webhook path, length ceiling, media and reactions
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use dravr_tronc::mcp::schema::{Tool, ToolResponse};
use dravr_tronc::{McpTool, ToolContext};
use serde_json::{json, Value};

use dravr_canot::channels::capabilities_for;

use crate::state::{ServerState, SharedState};

/// Lists all registered messaging channels with their descriptor metadata
pub struct ListChannels;

#[async_trait]
impl McpTool<ServerState> for ListChannels {
    fn definition(&self) -> Tool {
        Tool {
            name: "list_channels".to_owned(),
            description: "List all registered messaging channels and their capabilities".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
            annotations: None,
            // tronc 0.8.0 added these to `Tool`; neither applies to a
            // synchronous messaging tool with an untyped result.
            output_schema: None,
            execution: None,
        }
    }

    async fn execute(
        &self,
        state: &SharedState,
        _ctx: &ToolContext,
        _arguments: Value,
    ) -> ToolResponse {
        let registry = state.registry();
        let channel_types = registry.registered_channels();

        let channels: Vec<Value> = channel_types
            .iter()
            .filter_map(|ct| {
                let channel = registry.get(ct)?;
                let caps = capabilities_for(*ct);
                Some(json!({
                    "channel_type": ct.to_string(),
                    "name": channel.channel_type().to_string(),
                    "webhook_path": caps.map(|c| c.webhook_path),
                    "max_message_length": caps.map(|c| c.max_message_length),
                    "supports_media": caps.map(|c| c.supports_media),
                    "delivers_inbound_reactions": caps.map(|c| c.delivers_inbound_reactions),
                    "registered": true,
                }))
            })
            .collect();

        ToolResponse::text(
            json!({
                "channel_count": channels.len(),
                "channels": channels,
            })
            .to_string(),
        )
    }
}
