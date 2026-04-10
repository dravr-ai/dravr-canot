// ABOUTME: MCP tool for sending messages through a registered messaging channel
// ABOUTME: Accepts channel type, recipient, content, and optional reply-to for threaded messages
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use dravr_tronc::mcp::protocol::{CallToolResult, ToolDefinition};
use dravr_tronc::McpTool;
use serde_json::{json, Value};

use crate::state::{ServerState, SharedState};

/// Sends a text message through a registered messaging channel
pub struct SendMessage;

#[async_trait]
impl McpTool<ServerState> for SendMessage {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "send_message".to_owned(),
            description: "Send a text message through a registered messaging channel".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_type": {
                        "type": "string",
                        "description": "Channel platform: whatsapp, messenger, discord, slack, telegram",
                        "enum": ["whatsapp", "messenger", "discord", "slack", "telegram"]
                    },
                    "recipient_id": {
                        "type": "string",
                        "description": "Platform-specific recipient identifier"
                    },
                    "content": {
                        "type": "string",
                        "description": "Text message body to send"
                    },
                    "reply_to": {
                        "type": "string",
                        "description": "Optional message ID to reply to (for threading)"
                    }
                },
                "required": ["channel_type", "recipient_id", "content"]
            }),
        }
    }

    async fn execute(&self, state: &SharedState, arguments: Value) -> CallToolResult {
        let Some(channel_type_str) = arguments.get("channel_type").and_then(Value::as_str) else {
            return CallToolResult::error("Missing 'channel_type' argument".to_owned());
        };

        let channel_type: ChannelType = match channel_type_str.parse() {
            Ok(ct) => ct,
            Err(e) => return CallToolResult::error(format!("Invalid channel_type: {e}")),
        };

        let Some(recipient_id) = arguments.get("recipient_id").and_then(Value::as_str) else {
            return CallToolResult::error("Missing 'recipient_id' argument".to_owned());
        };

        let Some(content) = arguments.get("content").and_then(Value::as_str) else {
            return CallToolResult::error("Missing 'content' argument".to_owned());
        };

        let reply_to = arguments
            .get("reply_to")
            .and_then(Value::as_str)
            .map(String::from);

        let msg = OutgoingMessage {
            channel_type,
            recipient_id: recipient_id.to_owned(),
            content: MessageContent::Text {
                body: content.to_owned(),
            },
            correlation_id: uuid::Uuid::new_v4(),

            reply_to,
        };

        let guard = state.read().await;

        let Some(channel) = guard.registry().get(&channel_type) else {
            return CallToolResult::error(format!(
                "Channel '{channel_type}' is not registered in the server"
            ));
        };

        let Some(config) = guard.get_config(&channel_type) else {
            return CallToolResult::error(format!(
                "No configuration found for channel '{channel_type}'. Use set_channel_config first."
            ));
        };

        match channel.send(&msg, config).await {
            Ok(receipt) => CallToolResult::text(
                json!({
                    "status": "sent",
                    "message_id": receipt.message_id,
                    "channel_message_id": receipt.channel_message_id,
                    "delivery_status": receipt.status,
                    "timestamp": receipt.timestamp.to_rfc3339(),
                })
                .to_string(),
            ),
            Err(e) => CallToolResult::error(format!("Failed to send message: {e}")),
        }
    }
}
