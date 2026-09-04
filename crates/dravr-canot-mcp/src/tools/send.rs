// ABOUTME: MCP tool for sending messages through a registered messaging channel
// ABOUTME: Accepts channel type, recipient, content, and optional reply-to for threaded messages
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::turn::ConversationTurnId;
use dravr_tronc::mcp::schema::{Tool, ToolResponse};
use dravr_tronc::{McpTool, ToolContext};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::state::{ServerState, SharedState};

/// Sends a text message through a registered messaging channel
pub struct SendMessage;

#[async_trait]
impl McpTool<ServerState> for SendMessage {
    fn definition(&self) -> Tool {
        Tool {
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
                    },
                    "turn_id": {
                        "type": "string",
                        "format": "uuid",
                        "description": "Conversation-turn correlation identifier threaded from the inbound boundary. Must be a UUID; the caller is responsible for propagating (not regenerating) it."
                    }
                },
                "required": ["channel_type", "recipient_id", "content", "turn_id"]
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
        arguments: Value,
    ) -> ToolResponse {
        let Some(channel_type_str) = arguments.get("channel_type").and_then(Value::as_str) else {
            return ToolResponse::error("Missing 'channel_type' argument".to_owned());
        };

        let channel_type: ChannelType = match channel_type_str.parse() {
            Ok(ct) => ct,
            Err(e) => return ToolResponse::error(format!("Invalid channel_type: {e}")),
        };

        let Some(recipient_id) = arguments.get("recipient_id").and_then(Value::as_str) else {
            return ToolResponse::error("Missing 'recipient_id' argument".to_owned());
        };

        let Some(content) = arguments.get("content").and_then(Value::as_str) else {
            return ToolResponse::error("Missing 'content' argument".to_owned());
        };

        let reply_to = arguments
            .get("reply_to")
            .and_then(Value::as_str)
            .map(String::from);

        let Some(turn_id_str) = arguments.get("turn_id").and_then(Value::as_str) else {
            return ToolResponse::error(
                "Missing 'turn_id' argument: caller must provide the conversation-turn identifier"
                    .to_owned(),
            );
        };
        let turn_id = match Uuid::parse_str(turn_id_str) {
            Ok(id) => ConversationTurnId::from_uuid(id),
            Err(e) => return ToolResponse::error(format!("Invalid turn_id (expected UUID): {e}")),
        };

        let msg = OutgoingMessage {
            channel_type,
            recipient_id: recipient_id.to_owned(),
            content: MessageContent::Text {
                body: content.to_owned(),
            },
            turn_id,
            reply_to,
            thread_id: None,
        };

        let Some(channel) = state.registry().get(&channel_type) else {
            return ToolResponse::error(format!(
                "Channel '{channel_type}' is not registered in the server"
            ));
        };

        let Some(config) = state.get_config(&channel_type).await else {
            return ToolResponse::error(format!(
                "No configuration found for channel '{channel_type}'. Use set_channel_config first."
            ));
        };

        match channel.send(&msg, &config).await {
            Ok(receipt) => ToolResponse::text(
                json!({
                    "status": "sent",
                    "message_id": receipt.message_id,
                    "channel_message_id": receipt.channel_message_id,
                    "delivery_status": receipt.status,
                    "timestamp": receipt.timestamp.to_rfc3339(),
                    "turn_id": receipt.turn_id,
                })
                .to_string(),
            ),
            Err(e) => ToolResponse::error(format!("Failed to send message: {e}")),
        }
    }
}
