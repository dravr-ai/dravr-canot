// ABOUTME: Discord Bot API channel adapter module
// ABOUTME: Combines DiscordTransport (Ed25519 verification) with DiscordRenderer (embeds)
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

/// AG-UI progress status adapter (feature-gated)
#[cfg(feature = "agui")]
pub mod agui_status;
/// Discord Gateway WebSocket client for real-time message events
pub mod gateway;
/// Embed-based message renderer for Discord
pub mod renderer;
/// Ed25519 signature verification and webhook parsing for Discord
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

use self::renderer::DiscordRenderer;
use self::transport::DiscordTransport;

/// Discord channel adapter combining transport and renderer
pub struct DiscordChannel {
    /// Wire protocol adapter for Discord API
    transport: DiscordTransport,
    /// Embed-based message renderer
    renderer: DiscordRenderer,
}

impl DiscordChannel {
    /// Create a new Discord channel adapter
    #[must_use]
    pub fn new(public_key_hex: String, application_id: String) -> Self {
        Self {
            transport: DiscordTransport::new(public_key_hex, application_id),
            renderer: DiscordRenderer,
        }
    }
}

/// Discord channel metadata descriptor
pub struct DiscordDescriptor;

impl ChannelDescriptor for DiscordDescriptor {
    fn name(&self) -> &'static str {
        "discord"
    }
    fn display_name(&self) -> &'static str {
        "Discord"
    }
    fn channel_type(&self) -> ChannelType {
        ChannelType::Discord
    }
    fn webhook_path(&self) -> &'static str {
        "/api/messaging/webhook/discord"
    }
    fn supports_media(&self) -> bool {
        true
    }
    fn max_message_length(&self) -> usize {
        2000
    }
    fn signature_header(&self) -> &'static str {
        "x-signature-ed25519"
    }
}

#[async_trait]
impl MessagingChannel for DiscordChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::Discord
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

    async fn send_private_reply(
        &self,
        msg: &OutgoingMessage,
        recipient_user_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        // Discord can't post to a user id directly — open the 1:1 DM channel,
        // then send the reply there so only the caller sees it.
        let mut dm = msg.clone();
        dm.recipient_id = self
            .transport
            .open_dm_channel(recipient_user_id, config)
            .await?;
        dm.thread_id = None;
        self.send(&dm, config).await
    }
}
