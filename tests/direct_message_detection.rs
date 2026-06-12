// ABOUTME: Cross-channel integration tests for IncomingMessage.is_direct_message
// ABOUTME: Asserts each transport populates the flag from its native chat-kind signal
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

use dravr_canot::channels::discord::transport::DiscordTransport;
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

fn telegram_update(chat_type: &str) -> Value {
    json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "chat": {"id": 123, "type": chat_type},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "/coach",
        }
    })
}

fn telegram_callback_update(chat_type: &str) -> Value {
    json!({
        "update_id": 2,
        "callback_query": {
            "id": "cbq-1",
            "data": "/coach select abc",
            "from": {"id": 456, "first_name": "Chef"},
            "message": {
                "message_id": 42,
                "chat": {"id": 123, "type": chat_type},
            }
        }
    })
}

#[tokio::test]
async fn telegram_private_chat_is_direct_message() {
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&telegram_update("private")))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_direct_message);
}

#[tokio::test]
async fn telegram_group_chat_is_not_direct_message() {
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&telegram_update("group")))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].is_direct_message);
}

#[tokio::test]
async fn telegram_supergroup_is_not_direct_message() {
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&telegram_update("supergroup")))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].is_direct_message);
}

#[tokio::test]
async fn telegram_callback_query_preserves_chat_kind() {
    let transport = TelegramTransport::new("secret".to_owned());
    let private = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&telegram_callback_update("private")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(private[0].is_direct_message);

    let group = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&telegram_callback_update("supergroup")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(!group[0].is_direct_message);
}

// -------------------------------------------------------------------------
// Slack
// -------------------------------------------------------------------------

fn slack_event_payload(channel_type: &str, channel_id: &str) -> Value {
    json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "user": "U123",
            "text": "/coach",
            "channel": channel_id,
            "channel_type": channel_type,
            "ts": "1700000000.000100",
        }
    })
}

#[tokio::test]
async fn slack_im_channel_is_direct_message() {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_event_payload("im", "D123")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_direct_message);
}

#[tokio::test]
async fn slack_public_channel_is_not_direct_message() {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_event_payload("channel", "C123")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].is_direct_message);
}

#[tokio::test]
async fn slack_mpim_is_not_direct_message() {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_event_payload("mpim", "G123")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].is_direct_message);
}

// -------------------------------------------------------------------------
// Discord (interactions)
// -------------------------------------------------------------------------

fn discord_interaction(guild_id: Option<&str>) -> Value {
    let mut payload = json!({
        "id": "int-1",
        "type": 2,
        "channel_id": "chan-1",
        "data": {"options": [{"name": "text", "value": "/coach"}]},
        "user": {"id": "user-1", "username": "chef"},
    });
    if let Some(gid) = guild_id {
        payload["guild_id"] = json!(gid);
    }
    payload
}

fn discord_transport_for_tests() -> DiscordTransport {
    // 64 hex chars = 32 bytes, Ed25519 public key size; parse_inbound does not
    // verify signatures so a zeroed key is sufficient for body parsing.
    DiscordTransport::new("0".repeat(64), "app-1".to_owned())
}

#[tokio::test]
async fn discord_dm_interaction_is_direct_message() {
    let transport = discord_transport_for_tests();
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&discord_interaction(None)))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_direct_message);
}

#[tokio::test]
async fn discord_guild_interaction_is_not_direct_message() {
    let transport = discord_transport_for_tests();
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&discord_interaction(Some("guild-42"))),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(!messages[0].is_direct_message);
}

// -------------------------------------------------------------------------
// WhatsApp (always 1:1)
// -------------------------------------------------------------------------

#[tokio::test]
async fn whatsapp_messages_are_always_direct() {
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
    assert!(messages[0].is_direct_message);
}

// -------------------------------------------------------------------------
// Messenger (always 1:1)
// -------------------------------------------------------------------------

#[tokio::test]
async fn messenger_text_messages_are_always_direct() {
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
    assert!(messages[0].is_direct_message);
}

#[tokio::test]
async fn messenger_postback_events_are_always_direct() {
    let transport = MessengerTransport::new("secret".to_owned());
    let body = to_bytes(&json!({
        "entry": [{
            "messaging": [{
                "sender": {"id": "psid-1"},
                "recipient": {"id": "page-1"},
                "postback": {"payload": "/coach select abc", "mid": "m.xyz"}
            }]
        }]
    }));
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &body)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].is_direct_message);
}
