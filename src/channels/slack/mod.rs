// ABOUTME: Slack Events API channel adapter module
// ABOUTME: Combines SlackTransport (HMAC-SHA256 v0) with SlackRenderer (Block Kit)
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

/// AG-UI progress status adapter (feature-gated)
#[cfg(feature = "agui")]
pub mod agui_status;
/// Block Kit message renderer for Slack
pub mod renderer;
/// HMAC-SHA256 v0 signature verification and webhook parsing for Slack
pub mod transport;

use crate::error::MessagingResult;
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, IncomingMessage, OutgoingMessage,
};
use async_trait::async_trait;
use http::HeaderMap;
use serde_json::Value;

use crate::channel::MessagingChannel;
use crate::descriptor::ChannelDescriptor;
use crate::renderer::ResponseRenderer;
use crate::transport::TransportAdapter;
use crate::turn::ConversationTurnId;

use self::renderer::SlackRenderer;
use self::transport::SlackTransport;

/// Slack channel adapter combining transport and renderer
pub struct SlackChannel {
    /// Wire protocol adapter for Slack Events API
    transport: SlackTransport,
    /// Block Kit message renderer
    renderer: SlackRenderer,
}

impl SlackChannel {
    /// Create a new Slack channel adapter that drops every bot-authored message.
    #[must_use]
    pub fn new(signing_secret: String) -> Self {
        Self::with_allowed_bot_ids(signing_secret, Vec::new())
    }

    /// Create a Slack channel adapter that allow-lists the given bot IDs.
    ///
    /// See [`SlackTransport::with_allowed_bot_ids`] for the security caveats —
    /// allow-listed bots bypass the loop-prevention filter and are treated as
    /// real user input.
    #[must_use]
    pub fn with_allowed_bot_ids(signing_secret: String, allowed_bot_ids: Vec<String>) -> Self {
        Self {
            transport: SlackTransport::with_allowed_bot_ids(signing_secret, allowed_bot_ids),
            renderer: SlackRenderer,
        }
    }
}

/// Slack channel metadata descriptor
pub struct SlackDescriptor;

impl ChannelDescriptor for SlackDescriptor {
    fn name(&self) -> &'static str {
        "slack"
    }
    fn display_name(&self) -> &'static str {
        "Slack"
    }
    fn channel_type(&self) -> ChannelType {
        ChannelType::Slack
    }
    fn webhook_path(&self) -> &'static str {
        "/api/messaging/webhook/slack"
    }
    fn supports_media(&self) -> bool {
        true
    }
    fn max_message_length(&self) -> usize {
        40000
    }
    fn signature_header(&self) -> &'static str {
        "x-slack-signature"
    }
}

#[async_trait]
impl MessagingChannel for SlackChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Slack
    }

    fn verify_signature(&self, headers: &HeaderMap, body: &[u8]) -> MessagingResult<()> {
        self.transport.verify_signature(headers, body)
    }

    async fn receive(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<IncomingMessage>> {
        self.transport.parse_inbound(headers, body).await
    }

    fn render(&self, msg: &OutgoingMessage) -> MessagingResult<Value> {
        self.renderer.render(msg)
    }

    async fn send(
        &self,
        msg: &OutgoingMessage,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        let payload = self.render(msg)?;
        self.transport.send_raw(&payload, msg.turn_id, config).await
    }

    async fn send_raw(
        &self,
        payload: &Value,
        turn_id: ConversationTurnId,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        self.transport.send_raw(payload, turn_id, config).await
    }
}
