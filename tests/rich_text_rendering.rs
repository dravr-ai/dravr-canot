// ABOUTME: Per-channel golden snapshots for MessageContent::RichText rendering
// ABOUTME: Each renderer must translate the HTML subset into the platform's native markup
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! End-to-end rendering snapshots for `MessageContent::RichText`.
//!
//! A canonical rich text body containing both `<b>` and `<code>` is
//! fed through each of the five channel renderers (`SlackRenderer`,
//! `TelegramRenderer`, `DiscordRenderer`, `WhatsAppRenderer`,
//! `MessengerRenderer`) and the resulting JSON payload is asserted
//! against the expected platform-native shape.
//!
//! The same canonical input goes through every renderer so any
//! difference in the rendered payload reflects the renderer's
//! translation, not the input.

#![cfg(all(
    feature = "channel-telegram",
    feature = "channel-slack",
    feature = "channel-discord",
    feature = "channel-whatsapp",
    feature = "channel-messenger"
))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dravr_canot::channels::discord::renderer::DiscordRenderer;
use dravr_canot::channels::messenger::renderer::MessengerRenderer;
use dravr_canot::channels::slack::renderer::SlackRenderer;
use dravr_canot::channels::telegram::renderer::TelegramRenderer;
use dravr_canot::channels::whatsapp::renderer::WhatsAppRenderer;
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::renderer::ResponseRenderer;
use dravr_canot::turn::ConversationTurnId;
use serde_json::json;

/// Canonical privacy-status body — exactly the kind of string the
/// `/privacy` command emits across all locales. Contains both bold
/// and code spans so every translator gets exercised in one shot.
const RICH_BODY: &str = "Status is <b>enabled</b>. Use <code>/privacy off</code> to opt out.";

fn rich_msg(ct: ChannelType, recipient: &str) -> OutgoingMessage {
    OutgoingMessage {
        channel_type: ct,
        recipient_id: recipient.to_owned(),
        content: MessageContent::RichText {
            body: RICH_BODY.to_owned(),
        },
        turn_id: ConversationTurnId::new(),
        reply_to: None,
        thread_id: None,
    }
}

#[test]
fn telegram_renders_richtext_as_native_html() {
    let msg = rich_msg(ChannelType::Telegram, "123456789");
    let rendered = TelegramRenderer.render(&msg).unwrap();

    let expected = json!({
        "chat_id": "123456789",
        "text": "Status is <b>enabled</b>. Use <code>/privacy off</code> to opt out.",
        "parse_mode": "HTML"
    });

    assert_eq!(
        rendered, expected,
        "Telegram RichText must pass `<b>` and `<code>` through verbatim with parse_mode=HTML"
    );
}

#[test]
fn telegram_richtext_escapes_unsafe_chars_in_text_nodes() {
    // A coach typing "<100 bpm" must not break Telegram HTML parsing.
    let msg = OutgoingMessage {
        channel_type: ChannelType::Telegram,
        recipient_id: "123".to_owned(),
        content: MessageContent::RichText {
            body: "HR <100 bpm & pace > threshold".to_owned(),
        },
        turn_id: ConversationTurnId::new(),
        reply_to: None,
        thread_id: None,
    };
    let rendered = TelegramRenderer.render(&msg).unwrap();
    assert_eq!(
        rendered["text"], "HR &lt;100 bpm &amp; pace &gt; threshold",
        "Text nodes in RichText must be HTML-escaped so user content can't break parse_mode=HTML"
    );
}

#[test]
fn slack_renders_richtext_as_mrkdwn() {
    let msg = rich_msg(ChannelType::Slack, "C_RICH");
    let rendered = SlackRenderer.render(&msg).unwrap();

    let expected = json!({
        "channel": "C_RICH",
        "blocks": [{
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": "Status is *enabled*. Use `/privacy off` to opt out."
            }
        }]
    });

    assert_eq!(
        rendered, expected,
        "Slack RichText must translate `<b>` -> `*` and `<code>` -> `` ` `` (mrkdwn)"
    );
}

#[test]
fn whatsapp_renders_richtext_with_native_formatting() {
    let msg = rich_msg(ChannelType::WhatsApp, "+15551234567");
    let rendered = WhatsAppRenderer.render(&msg).unwrap();

    let expected = json!({
        "messaging_product": "whatsapp",
        "to": "+15551234567",
        "type": "text",
        "text": { "body": "Status is *enabled*. Use `/privacy off` to opt out." }
    });

    assert_eq!(
        rendered, expected,
        "WhatsApp RichText must translate `<b>` -> `*` and `<code>` -> `` ` ``"
    );
}

#[test]
fn discord_renders_richtext_as_markdown() {
    let msg = rich_msg(ChannelType::Discord, "987654321");
    let rendered = DiscordRenderer.render(&msg).unwrap();

    let expected = json!({
        "content": "Status is **enabled**. Use `/privacy off` to opt out.",
        "channel_id": "987654321"
    });

    assert_eq!(
        rendered, expected,
        "Discord RichText must translate `<b>` -> `**` and `<code>` -> `` ` ``"
    );
}

#[test]
fn messenger_renders_richtext_as_plain_text() {
    let msg = rich_msg(ChannelType::Messenger, "456");
    let rendered = MessengerRenderer.render(&msg).unwrap();

    let expected = json!({
        "recipient": { "id": "456" },
        "message": { "text": "Status is enabled. Use /privacy off to opt out." }
    });

    assert_eq!(
        rendered, expected,
        "Messenger RichText must strip all formatting tags — no native rich text format"
    );
}

#[test]
fn richtext_serde_round_trip_uses_rich_text_tag() {
    let content = MessageContent::RichText {
        body: "Status is <b>enabled</b>.".to_owned(),
    };
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(
        json,
        json!({
            "type": "rich_text",
            "body": "Status is <b>enabled</b>."
        }),
        "MessageContent::RichText must serialize with the `rich_text` snake_case tag for the host bridge",
    );
}
