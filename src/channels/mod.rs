// ABOUTME: Feature-gated channel adapter modules for each messaging platform
// ABOUTME: Each channel provides transport (wire protocol) and renderer (message formatting)
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use crate::descriptor::ChannelDescriptor;
use crate::models::ChannelType;

/// Whatsapp Business Cloud API adapter via Meta Graph API
#[cfg(feature = "channel-whatsapp")]
pub mod whatsapp;

/// Meta Messenger Platform adapter
#[cfg(feature = "channel-messenger")]
pub mod messenger;

/// Discord Bot API adapter with Ed25519 signature verification
#[cfg(feature = "channel-discord")]
pub mod discord;

/// Slack Events API adapter with Block Kit rendering
#[cfg(feature = "channel-slack")]
pub mod slack;

/// Telegram Bot API adapter with secret token verification
#[cfg(feature = "channel-telegram")]
pub mod telegram;

/// What a channel's descriptor declares about the wire it speaks.
///
/// The [`MessagingChannel`](crate::MessagingChannel) trait object a registry
/// hands back exposes none of this — the values live on the per-channel
/// [`ChannelDescriptor`] — so a caller holding only a [`ChannelType`] has no
/// way to read them. This is that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelCapabilities {
    /// Webhook URL path segment this channel is served on
    pub webhook_path: &'static str,
    /// Longest single message the platform accepts, in characters
    pub max_message_length: usize,
    /// Whether the channel renders media attachments natively
    pub supports_media: bool,
    /// Whether the channel's API delivers inbound reaction events
    pub delivers_inbound_reactions: bool,
}

/// Read a channel's declared capabilities from its descriptor.
///
/// Returns `None` when the channel's Cargo feature is not enabled, since the
/// descriptor is not compiled into that build.
#[must_use]
pub fn capabilities_for(channel: ChannelType) -> Option<ChannelCapabilities> {
    #[cfg_attr(
        not(any(
            feature = "channel-whatsapp",
            feature = "channel-messenger",
            feature = "channel-discord",
            feature = "channel-slack",
            feature = "channel-telegram"
        )),
        allow(dead_code)
    )]
    fn describe(d: &dyn ChannelDescriptor) -> ChannelCapabilities {
        ChannelCapabilities {
            webhook_path: d.webhook_path(),
            max_message_length: d.max_message_length(),
            supports_media: d.supports_media(),
            delivers_inbound_reactions: d.delivers_inbound_reactions(),
        }
    }

    match channel {
        ChannelType::WhatsApp => {
            #[cfg(feature = "channel-whatsapp")]
            {
                Some(describe(&whatsapp::WhatsAppDescriptor))
            }
            #[cfg(not(feature = "channel-whatsapp"))]
            {
                None
            }
        }
        ChannelType::Messenger => {
            #[cfg(feature = "channel-messenger")]
            {
                Some(describe(&messenger::MessengerDescriptor))
            }
            #[cfg(not(feature = "channel-messenger"))]
            {
                None
            }
        }
        ChannelType::Discord => {
            #[cfg(feature = "channel-discord")]
            {
                Some(describe(&discord::DiscordDescriptor))
            }
            #[cfg(not(feature = "channel-discord"))]
            {
                None
            }
        }
        ChannelType::Slack => {
            #[cfg(feature = "channel-slack")]
            {
                Some(describe(&slack::SlackDescriptor))
            }
            #[cfg(not(feature = "channel-slack"))]
            {
                None
            }
        }
        ChannelType::Telegram => {
            #[cfg(feature = "channel-telegram")]
            {
                Some(describe(&telegram::TelegramDescriptor))
            }
            #[cfg(not(feature = "channel-telegram"))]
            {
                None
            }
        }
    }
}
