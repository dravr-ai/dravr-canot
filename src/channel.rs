// ABOUTME: Core messaging channel trait combining transport and rendering
// ABOUTME: Each channel adapter implements this to handle inbound webhooks and outbound sends
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;
use http::HeaderMap;
use serde_json::Value;
use tracing::debug;

use crate::error::{MessagingError, MessagingResult};
use crate::models::{
    ChannelConfig, ChannelType, DeliveryReceipt, InboundReaction, IncomingMessage, OutgoingMessage,
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

    /// Parse inbound reaction events out of a webhook payload.
    ///
    /// Reactions are feedback on previously delivered messages, not new
    /// conversational input, so they surface as [`InboundReaction`] events
    /// separate from [`Self::receive`]'s message stream. Hosts call this
    /// alongside `receive` on the same verified webhook body for channels
    /// whose descriptor advertises
    /// [`delivers_inbound_reactions`](crate::descriptor::ChannelDescriptor::delivers_inbound_reactions).
    ///
    /// The default implementation returns an empty list: channels whose
    /// platform delivers no reaction webhook in this shape (`WhatsApp`
    /// Business Cloud, Messenger Platform) inherit it. Telegram, Slack,
    /// and Discord override it by delegating to their transport's
    /// `parse_reactions`.
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::InvalidPayload` if the payload cannot be
    /// parsed.
    async fn receive_reactions(
        &self,
        _headers: &HeaderMap,
        body: &[u8],
    ) -> MessagingResult<Vec<InboundReaction>> {
        debug!(
            body_len = body.len(),
            "channel delivers no inbound reaction events; none parsed"
        );
        Ok(vec![])
    }

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

    /// Remove a message from a room/chat on the channel.
    ///
    /// The platform calls this to delete a user's slash-command echo from a
    /// shared room (so the command stays private to the caller while the
    /// reply is routed to their DM). `conversation_id` is the channel-native
    /// room/chat id the message lives in; `channel_message_id` is the
    /// channel-native id of the message to remove.
    ///
    /// Telegram and Discord override this with a real delete call. The
    /// default reports [`MessagingError::OperationNotSupported`], because
    /// channels whose API cannot delete a member's message (`WhatsApp` and
    /// Messenger business APIs) never attempt one — and a caller that reads
    /// the outcome to decide what a room is now looking at must be able to
    /// tell "the message is gone" from "nothing was tried". Returning `Ok`
    /// for the second told every such caller the echo had been removed when
    /// it was still on screen.
    ///
    /// Deletion stays best-effort: Telegram refuses unless the bot is an
    /// admin with `can_delete_messages`, and callers treat every error here
    /// as non-fatal to the turn.
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::OperationNotSupported`] when the channel has
    /// no deletion API, and `MessagingError::ChannelApiError` /
    /// `DeliveryFailed` when an overriding channel's API rejects the request.
    async fn delete_message(
        &self,
        conversation_id: &str,
        channel_message_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<()> {
        debug!(
            channel = ?config.channel_type,
            conversation_id,
            channel_message_id,
            "delete_message not supported for this channel; nothing attempted"
        );
        Err(MessagingError::OperationNotSupported {
            channel: format!("{:?}", config.channel_type),
            operation: "message deletion".to_owned(),
        })
    }

    /// Deliver a reply privately to a single user instead of to the room the
    /// triggering message arrived in.
    ///
    /// Used so a slash command issued in a shared room is answered only to the
    /// caller — other members never see the response. `msg` carries the reply
    /// content (addressed to the room); `recipient_user_id` is the channel-
    /// native id of the user to deliver to privately.
    ///
    /// The default implementation re-addresses the message directly to the
    /// user so it lands in a 1:1 DM — correct for channels whose recipient id
    /// *is* the user id (Telegram, `WhatsApp`, Messenger). Slack overrides this
    /// with an ephemeral in-channel reply (`chat.postEphemeral`); Discord
    /// overrides it by opening a DM channel first.
    ///
    /// # Errors
    ///
    /// Returns `MessagingError::DeliveryFailed` / `ChannelApiError` when the
    /// underlying send fails.
    async fn send_private_reply(
        &self,
        msg: &OutgoingMessage,
        recipient_user_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        let dm = dm_redirect(msg, recipient_user_id);
        self.send(&dm, config).await
    }
}

/// Re-address a reply to a single user so it lands in a 1:1 DM.
///
/// The recipient becomes the user id and any room thread/topic is dropped (a
/// private chat has none). Content, channel, turn id, and `reply_to` are
/// preserved. Used by the default [`MessagingChannel::send_private_reply`] for
/// channels whose recipient id is the user id (Telegram, `WhatsApp`, Messenger).
#[must_use]
pub fn dm_redirect(msg: &OutgoingMessage, recipient_user_id: &str) -> OutgoingMessage {
    let mut dm = msg.clone();
    recipient_user_id.clone_into(&mut dm.recipient_id);
    dm.thread_id = None;
    dm
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::dm_redirect;
    use crate::models::{ChannelType, MessageContent, OutgoingMessage};
    use crate::turn::ConversationTurnId;

    fn sample_room_reply() -> OutgoingMessage {
        OutgoingMessage {
            channel_type: ChannelType::Telegram,
            recipient_id: "-1009999".to_owned(), // a group/room id
            content: MessageContent::Text {
                body: "your status".to_owned(),
            },
            turn_id: ConversationTurnId::new(),
            reply_to: None,
            thread_id: Some("17".to_owned()), // a forum topic in the room
        }
    }

    #[test]
    fn dm_redirect_targets_user_and_drops_room_thread() {
        let dm = dm_redirect(&sample_room_reply(), "user-555");
        // Reply now addressed to the user (1:1 DM), not the room.
        assert_eq!(dm.recipient_id, "user-555");
        // The room's forum topic doesn't exist in a DM.
        assert!(dm.thread_id.is_none());
    }

    #[test]
    fn dm_redirect_preserves_content_and_channel() {
        let original = sample_room_reply();
        let dm = dm_redirect(&original, "user-555");
        assert_eq!(dm.channel_type, original.channel_type);
        let body = match &dm.content {
            MessageContent::Text { body } => body.as_str(),
            _ => "",
        };
        assert_eq!(body, "your status");
    }
}
