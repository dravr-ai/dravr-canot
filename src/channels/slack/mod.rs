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

    async fn send_private_reply(
        &self,
        msg: &OutgoingMessage,
        recipient_user_id: &str,
        config: &ChannelConfig,
    ) -> MessagingResult<DeliveryReceipt> {
        // Slack's private primitive is an ephemeral message: visible only to
        // `recipient_user_id`, posted into the same channel the command came
        // from (msg.recipient_id), and never stored in channel history. Render
        // the normal payload, add the target user, and route it to
        // chat.postEphemeral.
        let payload = ephemeral_payload(self.render(msg)?, recipient_user_id);
        self.transport
            .send_ephemeral(&payload, msg.turn_id, config)
            .await
    }
}

/// Turn a rendered `chat.postMessage` payload into a `chat.postEphemeral` one.
///
/// Adds the `user` field; the `channel` field (the room the command came from)
/// is left as the renderer set it, so the ephemeral message appears in that
/// channel but only to `recipient_user_id`.
#[must_use]
pub fn ephemeral_payload(rendered: Value, recipient_user_id: &str) -> Value {
    let mut payload = rendered;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("user".to_owned(), Value::from(recipient_user_id));
    }
    payload
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::{ephemeral_payload, SlackChannel};
    use crate::channel::MessagingChannel;
    use crate::models::{ChannelType, MessageContent, OutgoingMessage};
    use crate::turn::ConversationTurnId;
    use serde_json::Value;

    #[test]
    fn ephemeral_payload_adds_user_and_keeps_channel() {
        // A rendered postMessage payload addresses the room via `channel`.
        let rendered = serde_json::json!({ "channel": "C-ROOM", "text": "your status" });
        let payload = ephemeral_payload(rendered, "U-CALLER");
        // postEphemeral needs `user`; the room channel is preserved so the
        // ephemeral reply shows in-channel but only to the caller.
        assert_eq!(payload["user"], Value::from("U-CALLER"));
        assert_eq!(payload["channel"], Value::from("C-ROOM"));
        assert_eq!(payload["text"], Value::from("your status"));
    }

    #[test]
    fn ephemeral_payload_channel_comes_from_rendered_recipient() {
        // The renderer addresses `channel` to the message recipient (the room
        // the slash command arrived in), so the ephemeral reply targets it.
        let channel = SlackChannel::new("signing-secret".to_owned());
        let msg = OutgoingMessage {
            channel_type: ChannelType::Slack,
            recipient_id: "C-ROOM".to_owned(),
            content: MessageContent::Text {
                body: "hi".to_owned(),
            },
            turn_id: ConversationTurnId::new(),
            reply_to: None,
            thread_id: None,
        };
        let rendered = channel.render(&msg).unwrap_or(Value::Null);
        let payload = ephemeral_payload(rendered, "U-CALLER");
        assert_eq!(payload["channel"], Value::from("C-ROOM"));
        assert_eq!(payload["user"], Value::from("U-CALLER"));
    }
}
