// ABOUTME: MCP tools for getting and setting channel-specific configuration
// ABOUTME: Manages API keys, webhook secrets, and other credentials per channel type
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use dravr_canot::models::{ChannelConfig, ChannelType};
use dravr_tronc::mcp::schema::{Tool, ToolResponse};
use dravr_tronc::{McpTool, ToolContext};
use serde_json::{json, Value};

use crate::state::{ServerState, SharedState};

/// Returns the current configuration for a given channel type
pub struct GetChannelConfig;

#[async_trait]
impl McpTool<ServerState> for GetChannelConfig {
    fn definition(&self) -> Tool {
        Tool {
            name: "get_channel_config".to_owned(),
            description: "Get the current configuration for a messaging channel".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_type": {
                        "type": "string",
                        "description": "Channel platform: whatsapp, messenger, discord, slack, telegram",
                        "enum": ["whatsapp", "messenger", "discord", "slack", "telegram"]
                    }
                },
                "required": ["channel_type"]
            }),
            annotations: None,
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

        state.get_config(&channel_type).await.map_or_else(
            || {
                ToolResponse::text(
                    json!({
                        "channel_type": channel_type.to_string(),
                        "configured": false,
                        "message": "No configuration set for this channel"
                    })
                    .to_string(),
                )
            },
            |config| {
                // Return config with secrets redacted for safety
                let redacted = json!({
                    "channel_type": channel_type.to_string(),
                    "is_active": config.is_active,
                    "has_api_key": config.api_key.is_some(),
                    "has_api_secret": config.api_secret.is_some(),
                    "has_webhook_secret": config.webhook_secret.is_some(),
                    "has_verify_token": config.verify_token.is_some(),
                    "has_bot_token": config.bot_token.is_some(),
                    "account_id": config.account_id,
                    "phone_number": config.phone_number,
                    "tenant_id": config.tenant_id,
                });
                ToolResponse::text(redacted.to_string())
            },
        )
    }
}

/// Sets or updates the configuration for a given channel type
pub struct SetChannelConfig;

#[async_trait]
impl McpTool<ServerState> for SetChannelConfig {
    fn definition(&self) -> Tool {
        Tool {
            name: "set_channel_config".to_owned(),
            description: "Set or update the configuration for a messaging channel".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_type": {
                        "type": "string",
                        "description": "Channel platform: whatsapp, messenger, discord, slack, telegram",
                        "enum": ["whatsapp", "messenger", "discord", "slack", "telegram"]
                    },
                    "api_key": {
                        "type": "string",
                        "description": "API key or access token for outbound API calls"
                    },
                    "api_secret": {
                        "type": "string",
                        "description": "API secret for signing outbound requests"
                    },
                    "webhook_secret": {
                        "type": "string",
                        "description": "Webhook secret for inbound signature verification"
                    },
                    "verify_token": {
                        "type": "string",
                        "description": "Webhook verification token (used during platform setup)"
                    },
                    "account_id": {
                        "type": "string",
                        "description": "Platform account identifier (e.g., Discord application ID)"
                    },
                    "phone_number": {
                        "type": "string",
                        "description": "Phone number identifier (WhatsApp)"
                    },
                    "bot_token": {
                        "type": "string",
                        "description": "Bot token for platforms using separate bot credentials"
                    },
                    "is_active": {
                        "type": "boolean",
                        "description": "Whether this channel configuration is active"
                    }
                },
                "required": ["channel_type"]
            }),
            annotations: None,
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

        let tenant_id = arguments
            .get("tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_owned();

        let config = ChannelConfig {
            id: format!("mcp-{channel_type_str}"),
            tenant_id,
            channel_type,
            api_key: arguments
                .get("api_key")
                .and_then(Value::as_str)
                .map(String::from),
            api_secret: arguments
                .get("api_secret")
                .and_then(Value::as_str)
                .map(String::from),
            webhook_secret: arguments
                .get("webhook_secret")
                .and_then(Value::as_str)
                .map(String::from),
            verify_token: arguments
                .get("verify_token")
                .and_then(Value::as_str)
                .map(String::from),
            account_id: arguments
                .get("account_id")
                .and_then(Value::as_str)
                .map(String::from),
            phone_number: arguments
                .get("phone_number")
                .and_then(Value::as_str)
                .map(String::from),
            bot_token: arguments
                .get("bot_token")
                .and_then(Value::as_str)
                .map(String::from),
            is_active: arguments
                .get("is_active")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        };

        state.set_config(channel_type, config).await;

        ToolResponse::text(
            json!({
                "channel_type": channel_type.to_string(),
                "status": "configured",
            })
            .to_string(),
        )
    }
}
