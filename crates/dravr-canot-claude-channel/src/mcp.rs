// ABOUTME: Minimal MCP stdio server implementing the Claude Channels protocol
// ABOUTME: Handles JSON-RPC 2.0 initialize/tools/call, sends channel notifications and permission verdicts
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

use dravr_canot::models::{ChannelConfig, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::ChannelRegistry;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, warn};

use crate::permission::PermissionRelay;

/// Events flowing from the webhook server into the MCP stdio loop
pub enum ChannelEvent {
    /// An inbound message from a messaging platform
    Message {
        /// Sender display name
        sender: String,
        /// Sender platform ID (for tracking last active sender)
        sender_id: String,
        /// Channel type string (e.g., "slack", "telegram")
        channel_type: String,
        /// Conversation/chat ID for reply routing
        chat_id: String,
        /// Message text content
        content: String,
    },
}

/// Thread-safe stdout writer for sending JSON-RPC notifications
pub type StdoutWriter = Arc<Mutex<io::Stdout>>;

/// Server name used in MCP initialize response and channel source attribute
const SERVER_NAME: &str = "dravr-canot";

/// MCP protocol version
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Run the MCP stdio server loop.
///
/// Reads JSON-RPC requests from stdin, handles initialize/tools/call,
/// and forwards channel events from the mpsc receiver as notifications.
pub async fn run(
    event_rx: mpsc::Receiver<ChannelEvent>,
    registry: Arc<ChannelRegistry>,
    configs: Arc<HashMap<ChannelType, ChannelConfig>>,
    permission_relay: Arc<PermissionRelay>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let stdout: StdoutWriter = Arc::new(Mutex::new(io::stdout()));
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut event_rx = event_rx;

    loop {
        tokio::select! {
            line_result = lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        handle_stdin_message(
                            &line,
                            &stdout,
                            Arc::clone(&registry),
                            Arc::clone(&configs),
                            Arc::clone(&permission_relay),
                        ).await;
                    }
                    Ok(None) => {
                        debug!("stdin closed, shutting down MCP server");
                        break;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to read from stdin");
                        break;
                    }
                }
            }
            Some(event) = event_rx.recv() => {
                handle_channel_event(&event, &stdout, &permission_relay).await;
            }
        }
    }

    Ok(())
}

/// Process a single JSON-RPC message from stdin
async fn handle_stdin_message(
    line: &str,
    stdout: &StdoutWriter,
    registry: Arc<ChannelRegistry>,
    configs: Arc<HashMap<ChannelType, ChannelConfig>>,
    permission_relay: Arc<PermissionRelay>,
) {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Invalid JSON from stdin");
            return;
        }
    };

    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let id = msg.get("id");

    match method {
        "initialize" => {
            if let Some(id) = id {
                let response = build_initialize_response(id);
                write_json(stdout, &response).await;
            }
        }
        "tools/list" => {
            if let Some(id) = id {
                let response = build_tools_list_response(id);
                write_json(stdout, &response).await;
            }
        }
        "tools/call" => {
            if let Some(id) = id {
                let tool_name = msg
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let arguments = msg
                    .get("params")
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                let response =
                    handle_tool_call(id, tool_name, &arguments, &registry, &configs).await;
                write_json(stdout, &response).await;
            }
        }
        "notifications/initialized" => {
            debug!("Client initialized notification received");
        }
        "notifications/claude/channel/permission_request" => {
            if let Some(params) = msg.get("params") {
                let request_id = params
                    .get("request_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let tool_name = params
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let description = params
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                permission_relay
                    .handle_request(request_id, tool_name, description, &registry, &configs)
                    .await;
            }
        }
        other => {
            if id.is_some() {
                // JSON-RPC request with unknown method — respond with error
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {other}")
                    }
                });
                write_json(stdout, &response).await;
            }
            // Unknown notifications are silently ignored per JSON-RPC spec
        }
    }
}

/// Handle a channel event by sending a notification to stdout
async fn handle_channel_event(
    event: &ChannelEvent,
    stdout: &StdoutWriter,
    permission_relay: &PermissionRelay,
) {
    match event {
        ChannelEvent::Message {
            sender,
            sender_id,
            channel_type,
            chat_id,
            content,
        } => {
            // Check if this is a permission verdict reply
            if let Some(verdict) = permission_relay.try_parse_verdict(content) {
                let notification = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/claude/channel/permission",
                    "params": {
                        "request_id": verdict.request_id,
                        "behavior": verdict.behavior
                    }
                });
                write_json(stdout, &notification).await;
                return;
            }

            // Track this sender as the last active for permission relay
            permission_relay
                .set_last_active_sender(channel_type, chat_id, sender_id)
                .await;

            // Forward as channel notification
            let notification = build_channel_notification(sender, channel_type, chat_id, content);
            write_json(stdout, &notification).await;
        }
    }
}

/// Build the MCP initialize response with Claude Channel capabilities
fn build_initialize_response(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {
                "name": SERVER_NAME,
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "experimental": {
                    "claude/channel": {},
                    "claude/channel/permission": {}
                },
                "tools": {}
            },
            "instructions": concat!(
                "Messages arrive as <channel source=\"dravr-canot\" sender=\"...\" ",
                "channel_type=\"...\" chat_id=\"...\">. ",
                "Reply with the reply tool, passing channel_type and chat_id from the tag."
            )
        }
    })
}

/// Build the tools/list response exposing the reply tool
fn build_tools_list_response(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "reply",
                    "description": "Send a message back to a messaging channel (Slack, Telegram, etc.)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "channel_type": {
                                "type": "string",
                                "description": "Channel platform: slack, telegram, discord, whatsapp, messenger",
                                "enum": ["slack", "telegram", "discord", "whatsapp", "messenger"]
                            },
                            "chat_id": {
                                "type": "string",
                                "description": "The conversation/chat ID from the inbound message tag"
                            },
                            "text": {
                                "type": "string",
                                "description": "The message text to send"
                            }
                        },
                        "required": ["channel_type", "chat_id", "text"]
                    }
                }
            ]
        }
    })
}

/// Handle a tools/call request for the reply tool
async fn handle_tool_call(
    id: &Value,
    tool_name: &str,
    arguments: &Value,
    registry: &ChannelRegistry,
    configs: &HashMap<ChannelType, ChannelConfig>,
) -> Value {
    if tool_name != "reply" {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "text", "text": format!("Unknown tool: {tool_name}")}],
                "isError": true
            }
        });
    }

    let channel_type_str = arguments
        .get("channel_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let chat_id = arguments
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let text = arguments.get("text").and_then(Value::as_str).unwrap_or("");

    if channel_type_str.is_empty() || chat_id.is_empty() || text.is_empty() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "text", "text": "Missing required arguments: channel_type, chat_id, text"}],
                "isError": true
            }
        });
    }

    let channel_type: ChannelType = match channel_type_str.parse() {
        Ok(ct) => ct,
        Err(e) => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{"type": "text", "text": format!("Invalid channel_type: {e}")}],
                    "isError": true
                }
            });
        }
    };

    let result_text = send_reply(&channel_type, chat_id, text, registry, configs).await;

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": result_text}]
        }
    })
}

/// Send a reply message through the channel adapter
pub async fn send_reply(
    channel_type: &ChannelType,
    chat_id: &str,
    text: &str,
    registry: &ChannelRegistry,
    configs: &HashMap<ChannelType, ChannelConfig>,
) -> String {
    let Some(adapter) = registry.get(channel_type) else {
        return format!("Channel '{channel_type}' is not registered");
    };

    let Some(config) = configs.get(channel_type) else {
        return format!("No configuration found for channel '{channel_type}'");
    };

    let msg = OutgoingMessage {
        channel_type: *channel_type,
        recipient_id: chat_id.to_owned(),
        content: MessageContent::Text {
            body: text.to_owned(),
        },
        correlation_id: uuid::Uuid::new_v4(),
        reply_to: None,
    };

    match adapter.send(&msg, config).await {
        Ok(receipt) => {
            format!(
                "Sent (message_id: {}, status: {:?})",
                receipt.message_id, receipt.status
            )
        }
        Err(e) => format!("Failed to send: {e}"),
    }
}

/// Build a `notifications/claude/channel` JSON-RPC notification
fn build_channel_notification(
    sender: &str,
    channel_type: &str,
    chat_id: &str,
    content: &str,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/claude/channel",
        "params": {
            "content": content,
            "meta": {
                "sender": sender,
                "channel_type": channel_type,
                "chat_id": chat_id
            }
        }
    })
}

/// Write a JSON value to stdout as a single line
async fn write_json(stdout: &StdoutWriter, value: &Value) {
    let mut out = stdout.lock().await;
    let serialized = serde_json::to_string(value).unwrap_or_default();
    if let Err(e) = writeln!(out, "{serialized}") {
        error!(error = %e, "Failed to write to stdout");
    }
    if let Err(e) = out.flush() {
        error!(error = %e, "Failed to flush stdout");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_response_has_channel_capabilities() {
        let resp = build_initialize_response(&json!(1));
        let result = &resp["result"];
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert!(result["capabilities"]["experimental"]["claude/channel"].is_object());
        assert!(result["capabilities"]["experimental"]["claude/channel/permission"].is_object());
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["instructions"].as_str().is_some());
    }

    #[test]
    fn tools_list_exposes_reply_tool() {
        let resp = build_tools_list_response(&json!(1));
        let tools = resp["result"]["tools"].as_array().expect("tools array"); // Safe: test assertion
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "reply");
        let required = tools[0]["inputSchema"]["required"]
            .as_array()
            .expect("required"); // Safe: test assertion
        let required_names: Vec<&str> = required.iter().filter_map(Value::as_str).collect();
        assert!(required_names.contains(&"channel_type"));
        assert!(required_names.contains(&"chat_id"));
        assert!(required_names.contains(&"text"));
    }

    #[test]
    fn channel_notification_format() {
        let notif = build_channel_notification("Alice", "slack", "C123", "hello world");
        assert_eq!(notif["method"], "notifications/claude/channel");
        assert_eq!(notif["params"]["content"], "hello world");
        assert_eq!(notif["params"]["meta"]["sender"], "Alice");
        assert_eq!(notif["params"]["meta"]["channel_type"], "slack");
        assert_eq!(notif["params"]["meta"]["chat_id"], "C123");
        // Must not have an id field (notifications are fire-and-forget)
        assert!(notif.get("id").is_none());
    }
}
