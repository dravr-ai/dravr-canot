// ABOUTME: Tests that capabilities_for reports each channel's real descriptor values
// ABOUTME: Guards against a caller re-fabricating capability metadata it cannot reach
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(feature = "all-channels")]

use dravr_canot::channels::capabilities_for;
use dravr_canot::models::ChannelType;

const ALL: [ChannelType; 5] = [
    ChannelType::WhatsApp,
    ChannelType::Messenger,
    ChannelType::Discord,
    ChannelType::Slack,
    ChannelType::Telegram,
];

#[test]
fn every_channel_reports_media_support() {
    // All five renderers return supports_media() == true. The MCP list_channels
    // tool used to emit a hardcoded false for every channel, telling an agent
    // the exact opposite of the truth about all of them.
    for channel in ALL {
        let caps = capabilities_for(channel)
            .unwrap_or_else(|| panic!("{channel:?} is compiled in under all-channels"));
        assert!(
            caps.supports_media,
            "{channel:?} renders media natively and must report it"
        );
    }
}

#[test]
fn message_ceilings_match_each_platform() {
    let ceiling = |c: ChannelType| capabilities_for(c).unwrap().max_message_length;
    assert_eq!(ceiling(ChannelType::Slack), 40_000);
    assert_eq!(ceiling(ChannelType::Telegram), 4096);
    assert_eq!(ceiling(ChannelType::WhatsApp), 4096);
    assert_eq!(ceiling(ChannelType::Discord), 2000);
    assert_eq!(ceiling(ChannelType::Messenger), 2000);
}

#[test]
fn every_channel_reports_its_own_webhook_path() {
    for channel in ALL {
        let caps = capabilities_for(channel).unwrap();
        assert!(
            caps.webhook_path.starts_with("/api/messaging/webhook/"),
            "{channel:?} webhook path: {}",
            caps.webhook_path
        );
    }
    assert_eq!(
        capabilities_for(ChannelType::Discord).unwrap().webhook_path,
        "/api/messaging/webhook/discord"
    );
}

#[test]
fn reaction_delivery_is_reported_per_channel() {
    // Telegram, Slack and Discord deliver inbound reactions; the two Meta
    // channels do not. Discord's are Gateway dispatch frames, which is why the
    // gateway must request the reaction intents for this to be true in fact.
    let delivers = |c: ChannelType| capabilities_for(c).unwrap().delivers_inbound_reactions;
    assert!(delivers(ChannelType::Telegram));
    assert!(delivers(ChannelType::Slack));
    assert!(delivers(ChannelType::Discord));
    assert!(!delivers(ChannelType::WhatsApp));
    assert!(!delivers(ChannelType::Messenger));
}
