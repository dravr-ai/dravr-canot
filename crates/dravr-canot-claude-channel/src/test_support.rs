// ABOUTME: Shared test helpers — mock channel adapter and fixtures for tests across modules
// ABOUTME: Provides MockAdapter implementing MessagingChannel plus sample IncomingMessage/ChannelConfig builders
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![cfg(test)]

use async_trait::async_trait;
use chrono::Utc;
use http::HeaderMap;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use dravr_canot::channel::MessagingChannel;
use dravr_canot::error::{MessagingError, MessagingResult};
use dravr_canot::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, DeliveryStatus, IncomingMessage, MessageContent,
    OutgoingMessage,
};

/// Configurable mock adapter for testing the webhook pipeline, MCP tool calls,
/// and permission relay without touching real messaging APIs.
pub struct MockAdapter {
    channel_type: ChannelType,
    inbound: Vec<IncomingMessage>,
    verify_ok: bool,
    parse_ok: bool,
    send_ok: bool,
    /// Captured outbound messages for assertions
    pub sent: Mutex<Vec<OutgoingMessage>>,
}

impl MockAdapter {
    pub fn new(channel_type: ChannelType) -> Self {
        Self {
            channel_type,
            inbound: Vec::new(),
            verify_ok: true,
            parse_ok: true,
            send_ok: true,
            sent: Mutex::new(Vec::new()),
        }
    }

    pub fn with_inbound(mut self, inbound: Vec<IncomingMessage>) -> Self {
        self.inbound = inbound;
        self
    }

    pub fn with_verify_failing(mut self) -> Self {
        self.verify_ok = false;
        self
    }

    pub fn with_parse_failing(mut self) -> Self {
        self.parse_ok = false;
        self
    }

    pub fn with_send_failing(mut self) -> Self {
        self.send_ok = false;
        self
    }
}

#[async_trait]
impl MessagingChannel for MockAdapter {
    fn channel_type(&self) -> ChannelType {
        self.channel_type
    }

    fn verify_signature(&self, _headers: &HeaderMap, _body: &[u8]) -> MessagingResult<()> {
        if self.verify_ok {
            Ok(())
        } else {
            Err(MessagingError::SignatureVerificationFailed {
                channel: self.channel_type.to_string(),
                reason: "mock signature failure".to_owned(),
            })
        }
    }

    async fn receive(
        &self,
        _headers: &HeaderMap,
        _body: &[u8],
    ) -> MessagingResult<Vec<IncomingMessage>> {
        if !self.parse_ok {
            return Err(MessagingError::InvalidPayload {
                channel: self.channel_type.to_string(),
                reason: "mock parse failure".to_owned(),
            });
        }
        Ok(self.inbound.clone())
    }

    fn render(&self, _msg: &OutgoingMessage) -> MessagingResult<Value> {
        Ok(json!({}))
    }

    async fn send(
        &self,
        msg: &OutgoingMessage,
        _config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        if !self.send_ok {
            return Err(MessagingError::DeliveryFailed {
                channel: self.channel_type.to_string(),
                reason: "mock send failure".to_owned(),
                retryable: false,
            });
        }
        self.sent.lock().await.push(msg.clone());
        Ok(DeliveryReceipt {
            message_id: Uuid::new_v4().to_string(),
            channel_message_id: Some("mock-id".to_owned()),
            status: DeliveryStatus::Sent,
            timestamp: Utc::now(),
        })
    }

    async fn send_raw(
        &self,
        _payload: &Value,
        _config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        Ok(DeliveryReceipt {
            message_id: Uuid::new_v4().to_string(),
            channel_message_id: None,
            status: DeliveryStatus::Sent,
            timestamp: Utc::now(),
        })
    }
}

/// Build a sample text `IncomingMessage` for a given channel type, sender, and conversation.
pub fn sample_incoming(
    channel_type: ChannelType,
    sender_id: &str,
    conversation_id: Option<&str>,
    text: &str,
) -> IncomingMessage {
    IncomingMessage {
        channel_type,
        sender_id: sender_id.to_owned(),
        sender_name: Some(format!("{sender_id}-name")),
        content: MessageContent::Text {
            body: text.to_owned(),
        },
        conversation_id: conversation_id.map(str::to_owned),
        channel_message_id: "mock-message-id".to_owned(),
        timestamp: Utc::now(),
        raw_payload: json!({}),
        correlation_id: Uuid::new_v4(),
        metadata: json!({}),
    }
}

/// Build a sample `ChannelConfig` with placeholder credentials for tests.
pub fn sample_config(channel_type: ChannelType) -> ChannelConfig {
    ChannelConfig {
        id: "cfg-1".to_owned(),
        tenant_id: "tenant-1".to_owned(),
        channel_type,
        api_key: Some("test-key".to_owned()),
        api_secret: None,
        webhook_secret: Some("test-secret".to_owned()),
        verify_token: None,
        account_id: None,
        phone_number: None,
        bot_token: Some("test-bot".to_owned()),
        is_active: true,
    }
}
