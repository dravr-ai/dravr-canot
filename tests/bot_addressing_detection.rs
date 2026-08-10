// ABOUTME: Cross-channel integration tests for IncomingMessage.addressed_to_bot
// ABOUTME: Asserts each transport detects bot mentions, replies-to-bot, and DM inherence
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(all(
    feature = "channel-telegram",
    feature = "channel-slack",
    feature = "channel-discord",
    feature = "channel-whatsapp",
    feature = "channel-messenger",
))]

use dravr_canot::channels::messenger::transport::MessengerTransport;
use dravr_canot::channels::slack::transport::SlackTransport;
use dravr_canot::channels::telegram::transport::TelegramTransport;
use dravr_canot::channels::whatsapp::transport::WhatsAppTransport;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

fn to_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_else(|e| panic!("serialize payload: {e}"))
}

// -------------------------------------------------------------------------
// Telegram
// -------------------------------------------------------------------------

/// Bot identity shared by every Telegram test in this file. The username
/// cache is process-wide, so all tests seed the SAME (id, username) pair —
/// parallel execution then cannot interfere.
const TG_BOT_ID: i64 = 999_001;
const TG_BOT_USERNAME: &str = "dravr_test_bot";

fn tg_transport() -> TelegramTransport {
    TelegramTransport::with_bot_identity("secret".to_owned(), TG_BOT_ID, TG_BOT_USERNAME)
}

fn tg_group_text(text: &str) -> Value {
    json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "chat": {"id": -100_123, "type": "supergroup", "title": "Club"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": text,
        }
    })
}

async fn tg_parse(transport: &TelegramTransport, update: &Value) -> bool {
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(update))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    messages[0].addressed_to_bot
}

#[tokio::test]
async fn telegram_group_plain_text_is_not_addressed() {
    assert!(!tg_parse(&tg_transport(), &tg_group_text("nice run everyone")).await);
}

#[tokio::test]
async fn telegram_group_username_mention_is_addressed() {
    assert!(tg_parse(&tg_transport(), &tg_group_text("@dravr_test_bot how am I doing?")).await);
}

#[tokio::test]
async fn telegram_mention_matching_is_case_insensitive() {
    assert!(tg_parse(&tg_transport(), &tg_group_text("hey @Dravr_Test_Bot thoughts?")).await);
}

#[tokio::test]
async fn telegram_longer_username_prefix_does_not_false_positive() {
    // "@dravr_test_botfan" contains "@dravr_test_bot" but addresses another user.
    assert!(!tg_parse(&tg_transport(), &tg_group_text("cc @dravr_test_botfan")).await);
}

#[tokio::test]
async fn telegram_bot_targeted_command_is_addressed() {
    assert!(tg_parse(&tg_transport(), &tg_group_text("/status@dravr_test_bot")).await);
}

#[tokio::test]
async fn telegram_reply_to_bot_message_is_addressed() {
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 43,
            "chat": {"id": -100_123, "type": "supergroup"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "tell me more",
            "reply_to_message": {
                "message_id": 42,
                "from": {"id": TG_BOT_ID, "is_bot": true, "first_name": "Dravr"},
                "text": "Your training load is trending up."
            }
        }
    });
    assert!(tg_parse(&tg_transport(), &update).await);
}

#[tokio::test]
async fn telegram_reply_to_human_message_is_not_addressed() {
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 43,
            "chat": {"id": -100_123, "type": "supergroup"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "agreed",
            "reply_to_message": {
                "message_id": 40,
                "from": {"id": 789, "first_name": "Phil"},
                "text": "easy week coming"
            }
        }
    });
    assert!(!tg_parse(&tg_transport(), &update).await);
}

#[tokio::test]
async fn telegram_text_mention_entity_of_bot_is_addressed() {
    // text_mention entities carry the user object (mentions of users without
    // a username) — the bot id match must not depend on the username cache.
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 44,
            "chat": {"id": -100_123, "type": "supergroup"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "Dravr what do you think?",
            "entities": [
                {"type": "text_mention", "offset": 0, "length": 5, "user": {"id": TG_BOT_ID, "is_bot": true}}
            ]
        }
    });
    assert!(tg_parse(&tg_transport(), &update).await);
}

#[tokio::test]
async fn telegram_caption_mention_is_addressed() {
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 45,
            "chat": {"id": -100_123, "type": "supergroup"},
            "from": {"id": 456, "first_name": "Chef"},
            "photo": [{"file_id": "f1", "width": 1, "height": 1}],
            "caption": "@dravr_test_bot analyze this route"
        }
    });
    let messages = tg_transport()
        .parse_inbound(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].addressed_to_bot);
}

#[tokio::test]
async fn telegram_without_bot_identity_never_detects_addressing() {
    // A transport built without identity (legacy constructor) cannot match
    // mentions — group messages parse with addressed_to_bot: false.
    let transport = TelegramTransport::new("secret".to_owned());
    assert!(!tg_parse(&transport, &tg_group_text("@dravr_test_bot hello")).await);
}

#[tokio::test]
async fn telegram_direct_message_is_inherently_addressed() {
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 46,
            "chat": {"id": 456, "type": "private"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "how was my week?",
        }
    });
    assert!(tg_parse(&tg_transport(), &update).await);
}

#[tokio::test]
async fn telegram_group_callback_query_is_addressed() {
    let update = json!({
        "update_id": 2,
        "callback_query": {
            "id": "cbq-1",
            "data": "/coach select abc",
            "from": {"id": 456, "first_name": "Chef"},
            "message": {
                "message_id": 42,
                "chat": {"id": -100_123, "type": "supergroup"},
            }
        }
    });
    assert!(tg_parse(&tg_transport(), &update).await);
}

// -------------------------------------------------------------------------
// Slack
// -------------------------------------------------------------------------

fn slack_channel_event(text: &str) -> Value {
    json!({
        "type": "event_callback",
        "authorizations": [{"user_id": "U777BOT", "is_bot": true}],
        "event": {
            "type": "message",
            "user": "U123",
            "text": text,
            "channel": "C123",
            "channel_type": "channel",
            "ts": "1700000000.000100",
        }
    })
}

async fn slack_parse(payload: &Value) -> bool {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    messages[0].addressed_to_bot
}

#[tokio::test]
async fn slack_channel_bot_mention_is_addressed() {
    assert!(slack_parse(&slack_channel_event("<@U777BOT> how is my recovery?")).await);
}

#[tokio::test]
async fn slack_channel_plain_text_is_not_addressed() {
    assert!(!slack_parse(&slack_channel_event("great intervals today")).await);
}

#[tokio::test]
async fn slack_channel_other_user_mention_is_not_addressed() {
    assert!(!slack_parse(&slack_channel_event("<@U888> nice pace!")).await);
}

#[tokio::test]
async fn slack_im_is_inherently_addressed() {
    let payload = json!({
        "type": "event_callback",
        "authorizations": [{"user_id": "U777BOT", "is_bot": true}],
        "event": {
            "type": "message",
            "user": "U123",
            "text": "how was my week?",
            "channel": "D123",
            "channel_type": "im",
            "ts": "1700000000.000100",
        }
    });
    assert!(slack_parse(&payload).await);
}

// -------------------------------------------------------------------------
// WhatsApp / Messenger (DM-only platforms)
// -------------------------------------------------------------------------

#[tokio::test]
async fn whatsapp_messages_are_always_addressed() {
    let transport = WhatsAppTransport::new("secret".to_owned());
    let body = to_bytes(&json!({
        "entry": [{
            "changes": [{
                "value": {
                    "messages": [{
                        "from": "15551234567",
                        "id": "wamid.abc",
                        "type": "text",
                        "text": {"body": "/coach"}
                    }],
                    "contacts": [{"wa_id": "15551234567", "profile": {"name": "Chef"}}]
                }
            }]
        }]
    }));
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &body)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].addressed_to_bot);
}

#[tokio::test]
async fn messenger_messages_are_always_addressed() {
    let transport = MessengerTransport::new("secret".to_owned());
    let body = to_bytes(&json!({
        "entry": [{
            "messaging": [{
                "sender": {"id": "psid-1"},
                "recipient": {"id": "page-1"},
                "message": {"mid": "m.abc", "text": "/coach"}
            }]
        }]
    }));
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &body)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].addressed_to_bot);
}
