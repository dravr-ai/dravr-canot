// ABOUTME: A Card body speaks the rich-text dialect — every channel renders it through its translator
// ABOUTME: Pins bold and code inside a card body on all five channels, and that the title stays plain
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dravr_canot::channels::discord::renderer::DiscordRenderer;
use dravr_canot::channels::messenger::renderer::MessengerRenderer;
use dravr_canot::channels::slack::renderer::SlackRenderer;
use dravr_canot::channels::telegram::renderer::TelegramRenderer;
use dravr_canot::channels::whatsapp::renderer::WhatsAppRenderer;
use dravr_canot::models::{CardAction, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::renderer::ResponseRenderer;
use dravr_canot::rich_text::{parse_markdown, render_rich_text};
use dravr_canot::turn::ConversationTurnId;
use serde_json::Value;

/// The body a host authored in markdown and converted for the channels —
/// the shape a `/help` listing or a `/privacy` reply arrives in.
const BODY_MARKDOWN: &str = "**Account**\n- `/privacy` — consent is **enabled**";

fn card(channel_type: ChannelType) -> OutgoingMessage {
    OutgoingMessage {
        channel_type,
        recipient_id: "16505551234".to_owned(),
        content: MessageContent::Card {
            title: "Commands <menu>".to_owned(),
            body: render_rich_text(&parse_markdown(BODY_MARKDOWN)),
            actions: vec![CardAction {
                label: "Status".to_owned(),
                action_type: "postback".to_owned(),
                value: "/status".to_owned(),
            }],
        },
        turn_id: ConversationTurnId::new(),
        reply_to: None,
        thread_id: None,
    }
}

fn render(renderer: &dyn ResponseRenderer, channel_type: ChannelType) -> Value {
    renderer
        .render(&card(channel_type))
        .unwrap_or_else(|e| panic!("render: {e}"))
}

#[test]
fn the_converted_body_is_the_dialect_every_renderer_reads() {
    assert_eq!(
        render_rich_text(&parse_markdown(BODY_MARKDOWN)),
        "<b>Account</b>\n- <code>/privacy</code> — consent is <b>enabled</b>"
    );
}

#[test]
fn telegram_card_body_keeps_its_formatting_and_escapes_only_the_title() {
    let text = render(&TelegramRenderer, ChannelType::Telegram)["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        text.starts_with("<b>Commands &lt;menu&gt;</b>\n\n"),
        "the title is plain text, so its angle brackets are escaped: {text}"
    );
    assert!(
        text.contains("<b>Account</b>") && text.contains("<code>/privacy</code>"),
        "the body's tags must reach Telegram as tags, not as escaped text: {text}"
    );
    assert!(
        !text.contains("&lt;b&gt;") && !text.contains("**"),
        "neither escaped tags nor raw markdown may reach the athlete: {text}"
    );
}

#[test]
fn slack_card_body_is_mrkdwn() {
    let payload = render(&SlackRenderer, ChannelType::Slack);
    assert_eq!(
        payload["blocks"][1]["text"]["text"],
        Value::from("*Account*\n- `/privacy` — consent is *enabled*")
    );
    assert_eq!(
        payload["blocks"][0]["text"]["text"],
        Value::from("Commands <menu>"),
        "the header block is plain_text and carries the title verbatim"
    );
}

#[test]
fn whatsapp_card_body_uses_its_own_formatting() {
    let payload = render(&WhatsAppRenderer, ChannelType::WhatsApp);
    assert_eq!(
        payload["interactive"]["body"]["text"],
        Value::from("*Account*\n- `/privacy` — consent is *enabled*")
    );
}

#[test]
fn discord_card_body_is_markdown() {
    let payload = render(&DiscordRenderer, ChannelType::Discord);
    assert_eq!(
        payload["embeds"][0]["description"],
        Value::from("**Account**\n- `/privacy` — consent is **enabled**")
    );
}

#[test]
fn messenger_card_body_is_plain_words() {
    let payload = render(&MessengerRenderer, ChannelType::Messenger);
    let element = &payload["message"]["attachment"]["payload"]["elements"][0];
    assert_eq!(
        element["subtitle"],
        Value::from("Account\n- /privacy — consent is enabled")
    );
    assert_eq!(element["title"], Value::from("Commands <menu>"));
}
