// ABOUTME: Telegram Bot API transport adapter for webhook parsing and message sending
// ABOUTME: Secret token header verification with constant-time comparison, Update payload parsing
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use crate::error::{MessagingError, MessagingResult};
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, DeliveryStatus, IncomingMessage, MessageContent,
};
use async_trait::async_trait;
use chrono::Utc;
use http::HeaderMap;
use serde_json::Value;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::http_client::api_client;

use crate::transport::TransportAdapter;
use crate::turn::ConversationTurnId;

/// Telegram Bot API transport adapter
///
/// Verification: `X-Telegram-Bot-Api-Secret-Token` header matched against
/// the configured webhook secret using constant-time comparison.
pub struct TelegramTransport {
    /// Shared HTTP client for outbound Bot API calls
    client: &'static reqwest::Client,
    /// Expected secret token for webhook verification
    webhook_secret: String,
}

impl TelegramTransport {
    /// Create a transport with the given webhook secret
    #[must_use]
    pub fn new(webhook_secret: String) -> Self {
        Self {
            client: api_client(),
            webhook_secret,
        }
    }
}

#[async_trait]
impl TransportAdapter for TelegramTransport {
    fn verify_signature(&self, headers: &HeaderMap, _body: &[u8]) -> MessagingResult<()> {
        let secret_header = headers
            .get("x-telegram-bot-api-secret-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| MessagingError::SignatureVerificationFailed {
                channel: "telegram".to_owned(),
                reason: "missing X-Telegram-Bot-Api-Secret-Token header".to_owned(),
            })?;

        // Constant-time comparison to prevent timing attacks
        let expected = self.webhook_secret.as_bytes();
        let received = secret_header.as_bytes();
        let equal: bool = subtle::ConstantTimeEq::ct_eq(received, expected).into();

        if equal {
            Ok(())
        } else {
            Err(MessagingError::SignatureVerificationFailed {
                channel: "telegram".to_owned(),
                reason: "secret token mismatch".to_owned(),
            })
        }
    }

    async fn parse_inbound(
        &self,
        _headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<IncomingMessage>> {
        // Webhook payload arrives as raw JSON. Operator-facing summary at
        // info; full body at trace so an operator following a typed message
        // through the stack can see exactly what Telegram delivered without
        // inspecting their reverse proxy.
        info!(body_len = body.len(), "telegram parse_inbound received");
        if tracing::enabled!(tracing::Level::TRACE) {
            trace!(
                body = %String::from_utf8_lossy(body),
                "telegram inbound body"
            );
        }

        let update: Value =
            serde_json::from_slice(body).map_err(|e| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: format!("invalid JSON: {e}"),
            })?;

        // Telegram sends one Update per webhook — check for callback_query first (button taps)
        if let Some(callback) = update.get("callback_query") {
            return Ok(parse_callback_query(callback, &update));
        }

        let Some(message) = update.get("message") else {
            debug!("Telegram update without message or callback_query field");
            return Ok(vec![]);
        };

        // Skip Telegram service messages (member added/removed, group created, etc.)
        // These are not user-authored content and should not be dispatched to the LLM.
        if is_service_message(message) {
            debug!("Skipping Telegram service message");
            return Ok(vec![]);
        }

        let chat_id = message
            .pointer("/chat/id")
            .and_then(Value::as_i64)
            .ok_or_else(|| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: "missing chat.id".to_owned(),
            })?;

        // Telegram chat.type is one of "private", "group", "supergroup",
        // "channel" (Bot API getUpdates spec). Only "private" is a 1:1 DM.
        let is_direct_message =
            message.pointer("/chat/type").and_then(Value::as_str) == Some("private");

        // Telegram exposes chat.title for group / supergroup / channel
        // (Bot API). Private chats carry first_name/last_name instead and
        // produce None here.
        let chat_title = message
            .pointer("/chat/title")
            .and_then(Value::as_str)
            .map(str::to_owned);

        let from_id = message
            .pointer("/from/id")
            .and_then(Value::as_i64)
            .unwrap_or(chat_id);

        let from_name = message
            .pointer("/from/first_name")
            .and_then(Value::as_str)
            .map(str::to_owned);

        let message_id = message
            .get("message_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        let content = message.get("text").and_then(Value::as_str).map_or_else(
            || parse_non_text_content(message),
            |text| MessageContent::Text {
                body: text.to_owned(),
            },
        );

        // Extract forum topic thread ID for groups with Topics enabled
        let metadata = message
            .get("message_thread_id")
            .and_then(Value::as_i64)
            .map_or(
                Value::Null,
                |thread_id| serde_json::json!({ "message_thread_id": thread_id }),
            );

        let incoming = IncomingMessage {
            channel_type: ChannelType::Telegram,
            sender_id: from_id.to_string(),
            sender_name: from_name,
            content,
            conversation_id: Some(chat_id.to_string()),
            chat_title,
            channel_message_id: message_id.to_string(),
            timestamp: Utc::now(),
            raw_payload: update,
            turn_id: ConversationTurnId::new(),
            is_direct_message,
            metadata,
        };

        info!(
            turn_id = %incoming.turn_id,
            chat_id,
            message_id,
            is_direct_message,
            content_kind = match &incoming.content {
                MessageContent::Text { .. } => "text",
                _ => "non_text",
            },
            "telegram inbound message parsed"
        );

        Ok(vec![incoming])
    }

    async fn send_raw(
        &self,
        payload: &Value,
        turn_id: ConversationTurnId,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        let bot_token =
            config
                .bot_token
                .as_deref()
                .ok_or_else(|| MessagingError::ChannelNotConfigured {
                    channel: "telegram".to_owned(),
                })?;

        let method = resolve_bot_api_method(payload);
        let url = format!("https://api.telegram.org/bot{bot_token}/{method}");

        let response = self
            .client
            .post(&url)
            .json(payload)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "telegram".to_owned(),
                reason: format!("HTTP request failed: {e}"),
                retryable: true,
            })?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_owned());
            return Err(MessagingError::ChannelApiError {
                channel: "telegram".to_owned(),
                status_code: status,
                message: body_text,
            });
        }

        let result: Value = response
            .json()
            .await
            .map_err(|e| MessagingError::InvalidPayload {
                channel: "telegram".to_owned(),
                reason: format!("invalid response JSON: {e}"),
            })?;

        let channel_message_id = result
            .pointer("/result/message_id")
            .and_then(Value::as_i64)
            .map(|id| id.to_string());

        Ok(DeliveryReceipt {
            message_id: Uuid::new_v4().to_string(),
            channel_message_id,
            status: DeliveryStatus::Sent,
            timestamp: Utc::now(),
            turn_id,
        })
    }
}

impl TelegramTransport {
    /// Delete a message from a chat via the Telegram `deleteMessage` Bot API.
    ///
    /// `chat_id` is the room/chat the message lives in and `message_id` is
    /// the Telegram message id to remove. Telegram requires `message_id` to
    /// be an integer and only honours the call when the bot is an admin with
    /// `can_delete_messages` in the chat; otherwise it returns HTTP 400 and
    /// this surfaces a [`MessagingError::ChannelApiError`].
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::ChannelNotConfigured`] when no bot token is
    /// present, [`MessagingError::DeliveryFailed`] when `message_id` is not a
    /// valid integer or the request can't be sent, and
    /// [`MessagingError::ChannelApiError`] when Telegram rejects the deletion.
    pub async fn delete_message(
        &self,
        chat_id: &str,
        message_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<()> {
        let bot_token =
            config
                .bot_token
                .as_deref()
                .ok_or_else(|| MessagingError::ChannelNotConfigured {
                    channel: "telegram".to_owned(),
                })?;

        // Telegram's deleteMessage requires an integer message_id; the
        // platform stores it as a string, so parse it back here.
        let message_id_int: i64 =
            message_id
                .parse()
                .map_err(|e| MessagingError::DeliveryFailed {
                    channel: "telegram".to_owned(),
                    reason: format!("invalid message_id '{message_id}': {e}"),
                    retryable: false,
                })?;

        // chat_id is "Integer or String" in the Bot API. Numeric ids
        // (including negative supergroup ids) go as integers; the rare
        // @username form stays a string.
        let chat_id_value = chat_id_to_value(chat_id);

        let payload = serde_json::json!({
            "chat_id": chat_id_value,
            "message_id": message_id_int,
        });

        let url = format!("https://api.telegram.org/bot{bot_token}/deleteMessage");
        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "telegram".to_owned(),
                reason: format!("HTTP request failed: {e}"),
                retryable: true,
            })?;

        if response.status().is_success() {
            debug!(chat_id, message_id, "telegram message deleted");
            return Ok(());
        }

        let status = response.status().as_u16();
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown".to_owned());
        Err(MessagingError::ChannelApiError {
            channel: "telegram".to_owned(),
            status_code: status,
            message: body_text,
        })
    }
}

/// Coerce a Telegram chat id string into the Bot API's "Integer or String"
/// shape: numeric ids (including negative supergroup ids) become a JSON
/// number, everything else (e.g. an `@channelusername`) stays a string.
fn chat_id_to_value(chat_id: &str) -> Value {
    chat_id
        .parse::<i64>()
        .map_or_else(|_| Value::from(chat_id), Value::from)
}

/// Resolve the Telegram Bot API method from the rendered payload shape
///
/// The renderer produces payloads with different keys depending on content type:
/// - `"photo"` key → `sendPhoto`
/// - `"latitude"` key → `sendLocation`
/// - default → `sendMessage` (text and card content)
fn resolve_bot_api_method(payload: &Value) -> &'static str {
    if payload.get("photo").is_some() {
        "sendPhoto"
    } else if payload.get("latitude").is_some() {
        "sendLocation"
    } else {
        "sendMessage"
    }
}

/// Parse a Telegram `callback_query` update (inline keyboard button tap)
///
/// The `callback_query` contains the button's `data` field and the original message context.
fn parse_callback_query(callback: &Value, update: &Value) -> Vec<IncomingMessage> {
    let from_id = callback
        .pointer("/from/id")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let from_name = callback
        .pointer("/from/first_name")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let chat_id = callback
        .pointer("/message/chat/id")
        .and_then(Value::as_i64)
        .unwrap_or(from_id);

    // Mirror the message-path rule: callback_query messages carry the same
    // chat.type shape inside /message/chat/type.
    let is_direct_message = callback
        .pointer("/message/chat/type")
        .and_then(Value::as_str)
        == Some("private");

    let chat_title = callback
        .pointer("/message/chat/title")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let callback_data = callback.get("data").and_then(Value::as_str).unwrap_or("");
    let callback_id = callback.get("id").and_then(Value::as_str).unwrap_or("0");

    // Extract forum topic thread ID from the original message context
    let metadata = callback
        .pointer("/message/message_thread_id")
        .and_then(Value::as_i64)
        .map_or(
            Value::Null,
            |thread_id| serde_json::json!({ "message_thread_id": thread_id }),
        );

    let incoming = IncomingMessage {
        channel_type: ChannelType::Telegram,
        sender_id: from_id.to_string(),
        sender_name: from_name,
        content: MessageContent::Text {
            body: callback_data.to_owned(),
        },
        conversation_id: Some(chat_id.to_string()),
        chat_title,
        channel_message_id: callback_id.to_owned(),
        timestamp: Utc::now(),
        raw_payload: update.clone(),
        turn_id: ConversationTurnId::new(),
        is_direct_message,
        metadata,
    };

    vec![incoming]
}

/// Telegram service messages that are not user-authored content
///
/// These updates carry metadata about group changes (member joins/leaves,
/// title/photo changes, migrations) and should be silently ignored rather
/// than dispatched to the LLM pipeline.
const SERVICE_MESSAGE_FIELDS: &[&str] = &[
    "new_chat_members",
    "left_chat_member",
    "new_chat_title",
    "new_chat_photo",
    "delete_chat_photo",
    "group_chat_created",
    "supergroup_chat_created",
    "channel_chat_created",
    "pinned_message",
    "migrate_to_chat_id",
    "migrate_from_chat_id",
    "message_auto_delete_timer_changed",
    "forum_topic_created",
    "forum_topic_closed",
    "forum_topic_reopened",
    "video_chat_started",
    "video_chat_ended",
    "video_chat_participants_invited",
];

/// Check if a Telegram message is a service notification rather than user content
fn is_service_message(message: &Value) -> bool {
    SERVICE_MESSAGE_FIELDS
        .iter()
        .any(|field| message.get(field).is_some())
}

/// Parse non-text message content (location, photo, or unsupported)
fn parse_non_text_content(message: &Value) -> MessageContent {
    if let Some(location) = message.get("location") {
        let lat = location
            .get("latitude")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let lon = location
            .get("longitude")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        return MessageContent::Location {
            latitude: lat,
            longitude: lon,
        };
    }

    if message.get("photo").is_some() {
        // Photo array — take the largest (last) entry
        let photo_url = message
            .get("photo")
            .and_then(Value::as_array)
            .and_then(|arr| arr.last())
            .and_then(|p| p.get("file_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let caption = message
            .get("caption")
            .and_then(Value::as_str)
            .map(str::to_owned);
        return MessageContent::Media {
            url: photo_url,
            mime_type: "image/jpeg".to_owned(),
            caption,
        };
    }

    warn!("Telegram message with unsupported content type");
    MessageContent::Text {
        body: "[unsupported message type]".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::chat_id_to_value;
    use serde_json::Value;

    #[test]
    fn numeric_chat_id_becomes_a_json_number() {
        // Private/user chats use small positive ids.
        assert_eq!(chat_id_to_value("99"), Value::from(99_i64));
    }

    #[test]
    fn negative_supergroup_id_stays_a_json_number() {
        // Supergroup chat ids are large negative integers; they must remain
        // numbers so Telegram routes deleteMessage to the right chat.
        assert_eq!(
            chat_id_to_value("-1001234567890"),
            Value::from(-1_001_234_567_890_i64)
        );
    }

    #[test]
    fn username_chat_id_stays_a_string() {
        // The @channelusername form can't parse as an integer and is sent
        // verbatim as a string.
        assert_eq!(
            chat_id_to_value("@dravrchannel"),
            Value::from("@dravrchannel")
        );
    }
}
