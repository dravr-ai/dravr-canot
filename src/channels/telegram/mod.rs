// ABOUTME: Telegram Bot API channel adapter module
// ABOUTME: Combines TelegramTransport (secret token) with TelegramRenderer (HTML parse mode)
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

/// AG-UI progress status adapter (feature-gated)
#[cfg(feature = "agui")]
pub mod agui_status;
/// HTML parse mode message renderer for Telegram
pub mod renderer;
/// Secret token verification and webhook parsing for Telegram
pub mod transport;

use crate::error::MessagingResult;
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, InboundReaction, IncomingMessage, OutgoingMessage,
};
use async_trait::async_trait;
use http::HeaderMap;
use serde_json::Value;

use crate::channel::MessagingChannel;
use crate::descriptor::ChannelDescriptor;
use crate::renderer::ResponseRenderer;
use crate::transport::TransportAdapter;
use crate::turn::ConversationTurnId;

use self::renderer::TelegramRenderer;
use self::transport::TelegramTransport;

/// Telegram channel adapter combining transport and renderer
pub struct TelegramChannel {
    /// Wire protocol adapter for Telegram Bot API
    transport: TelegramTransport,
    /// HTML-based message renderer
    renderer: TelegramRenderer,
}

impl TelegramChannel {
    /// Create a new Telegram channel adapter without bot identity
    /// (bot-addressing detection disabled).
    #[must_use]
    pub fn new(webhook_secret: String) -> Self {
        Self {
            transport: TelegramTransport::new(webhook_secret),
            renderer: TelegramRenderer,
        }
    }

    /// Create an adapter that detects bot-addressed messages: the bot id is
    /// derived from the token prefix, the username lazily via `getMe`.
    #[must_use]
    pub fn with_bot_token(webhook_secret: String, bot_token: Option<String>) -> Self {
        Self {
            transport: TelegramTransport::with_bot_token(webhook_secret, bot_token),
            renderer: TelegramRenderer,
        }
    }
}

/// Telegram channel metadata descriptor
pub struct TelegramDescriptor;

impl ChannelDescriptor for TelegramDescriptor {
    fn name(&self) -> &'static str {
        "telegram"
    }
    fn display_name(&self) -> &'static str {
        "Telegram"
    }
    fn channel_type(&self) -> ChannelType {
        ChannelType::Telegram
    }
    fn webhook_path(&self) -> &'static str {
        "/api/messaging/webhook/telegram"
    }
    fn supports_media(&self) -> bool {
        true
    }
    /// Telegram delivers `message_reaction` updates, but only when the
    /// bot's webhook is registered with `allowed_updates` including
    /// `"message_reaction"` — the Bot API excludes it from the default
    /// update set.
    fn delivers_inbound_reactions(&self) -> bool {
        true
    }
    fn max_message_length(&self) -> usize {
        4096
    }
    fn signature_header(&self) -> &'static str {
        "x-telegram-bot-api-secret-token"
    }
}

#[async_trait]
impl MessagingChannel for TelegramChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Telegram
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

    async fn receive_reactions(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<InboundReaction>> {
        self.transport.parse_reactions(headers, body).await
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

    async fn delete_message(
        &self,
        conversation_id: &str,
        channel_message_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<()> {
        self.transport
            .delete_message(conversation_id, channel_message_id, config)
            .await
    }
}
