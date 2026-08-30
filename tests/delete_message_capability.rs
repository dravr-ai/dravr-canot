// ABOUTME: Asserts a channel with no deletion API reports it rather than faking success
// ABOUTME: Covers the MessagingChannel::delete_message default and Telegram's override
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![cfg(feature = "channel-whatsapp")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dravr_canot::channel::MessagingChannel;
use dravr_canot::channels::whatsapp::WhatsAppChannel;
use dravr_canot::error::MessagingError;
use dravr_canot::models::{ChannelConfig, ChannelType};

fn whatsapp_config() -> ChannelConfig {
    ChannelConfig {
        id: "cfg-delete".into(),
        tenant_id: "tenant-1".into(),
        channel_type: ChannelType::WhatsApp,
        api_key: Some("token".into()),
        api_secret: None,
        webhook_secret: Some("secret".into()),
        verify_token: None,
        account_id: None,
        phone_number: Some("+15550000000".into()),
        bot_token: None,
        is_active: true,
    }
}

/// A channel whose API cannot delete a member's message must say so.
///
/// It previously returned `Ok(())`, which reads as "the message is gone".
/// The platform deletes a slash-command echo from a shared room and uses the
/// outcome to decide whether the room still needs to be told anything — so a
/// success it did not earn left the command on screen with silence under it,
/// which is the exact defect the deletion exists to prevent.
#[tokio::test]
async fn channel_without_deletion_api_reports_not_supported() {
    let channel = WhatsAppChannel::new("secret".into());
    let config = whatsapp_config();

    let result = channel.delete_message("room-1", "message-1", &config).await;

    match result {
        Err(MessagingError::OperationNotSupported { channel, operation }) => {
            assert_eq!(operation, "message deletion");
            assert!(
                channel.to_lowercase().contains("whatsapp"),
                "error should name the channel that lacks the operation, got {channel}"
            );
        }
        Err(other) => panic!("expected OperationNotSupported, got: {other}"),
        Ok(()) => panic!("a channel that deleted nothing must not report success"),
    }
}
