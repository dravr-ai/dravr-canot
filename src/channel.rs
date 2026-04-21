// ABOUTME: Core messaging channel trait combining transport and rendering
// ABOUTME: Each channel adapter implements this to handle inbound webhooks and outbound sends
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use http::HeaderMap;
use serde_json::Value;

use crate::error::MessagingResult;
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, IncomingMessage, OutgoingMessage,
};
use crate::turn::ConversationTurnId;

/// Unified messaging channel interface
///
/// Each channel adapter (Whatsapp, Slack, etc.) implements this trait to provide:
/// - Webhook signature verification
/// - Inbound message parsing
/// - Outbound message delivery with channel-specific formatting
#[async_trait]
pub trait MessagingChannel: Send + Sync {
    /// Channel type identifier
    fn channel_type(&self) -> ChannelType;

    /// Verify webhook signature using channel-specific algorithm
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::SignatureVerificationFailed` if the signature is
    /// invalid or missing.
    fn verify_signature(&self, headers: &HeaderMap, body: &[u8]) -> MessagingResult<()>;

    /// Parse inbound webhook payload into structured messages
    ///
    /// A single webhook may contain multiple messages (e.g., Messenger batches).
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::InvalidPayload` if the payload cannot be parsed.
    async fn receive(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<IncomingMessage>>;

    /// Render an outbound message to the channel's native payload format
    ///
    /// Returns the channel-specific JSON payload without sending it.
    /// Used by the outbound retry queue to store the rendered payload for later delivery.
    ///
    /// # Errors
    ///
    /// Returns `MessagingError` if the content cannot be rendered for this channel.
    fn render(&self, msg: &OutgoingMessage) -> MessagingResult<Value>;

    /// Format and send an outbound message through the channel
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::DeliveryFailed` or `MessagingError::ChannelApiError`
    /// if the send fails.
    async fn send(
        &self,
        msg: &OutgoingMessage,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt>;

    /// Send a pre-rendered payload directly to the channel API.
    ///
    /// Used by the outbound retry worker where payloads are already rendered
    /// and stored in the queue. `turn_id` is the conversation-turn
    /// identifier carried from the originating outbound message; the
    /// retry worker pulls it from the queue entry and passes it through
    /// so the resulting [`DeliveryReceipt`] stays keyed to the same turn.
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::DeliveryFailed` if the send fails.
    async fn send_raw(
        &self,
        payload: &Value,
        turn_id: ConversationTurnId,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt>;
}
