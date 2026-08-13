// ABOUTME: Cross-transport tests for IncomingMessage.chat_title population
// ABOUTME: Asserts each adapter resolves the human-readable chat title where the wire format exposes one
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
use dravr_canot::models::MessageContent;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

fn to_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_else(|e| panic!("serialize payload: {e}"))
}

// -------------------------------------------------------------------------
// Telegram — chat.title is the canonical title source for groups, supergroups,
// and channels. Private chats carry first_name/last_name instead.
// -------------------------------------------------------------------------

#[tokio::test]
async fn telegram_group_message_carries_chat_title() {
    let payload = json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "chat": {"id": -100_123, "type": "supergroup", "title": "Tortues Course"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "Bonjour le groupe",
        }
    });
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].chat_title.as_deref(), Some("Tortues Course"));
}

#[tokio::test]
async fn telegram_private_chat_has_no_chat_title() {
    let payload = json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "chat": {"id": 999, "type": "private"},
            "from": {"id": 999, "first_name": "Chef"},
            "text": "Salut",
        }
    });
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].chat_title.is_none());
}

#[tokio::test]
async fn telegram_callback_query_carries_chat_title_for_groups() {
    let payload = json!({
        "update_id": 2,
        "callback_query": {
            "id": "cbq-1",
            "data": "/coach select abc",
            "from": {"id": 456, "first_name": "Chef"},
            "message": {
                "message_id": 42,
                "chat": {"id": -100_123, "type": "supergroup", "title": "Tortues Course"},
            }
        }
    });
    let transport = TelegramTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].chat_title.as_deref(), Some("Tortues Course"));
}

// -------------------------------------------------------------------------
// Discord — Interactions API embeds the channel object on the payload (v10).
// Gateway MESSAGE_CREATE only carries channel_id; resolution is host-side.
// -------------------------------------------------------------------------

#[tokio::test]
async fn discord_interaction_carries_channel_name() {
    let payload = json!({
        "type": 2,
        "id": "interaction-1",
        "guild_id": "guild-1",
        "channel_id": "channel-1",
        "channel": {"id": "channel-1", "name": "general"},
        "data": {"name": "coach", "options": [{"name": "subcommand", "value": "Bonjour"}]},
        "member": {"user": {"id": "user-1", "username": "Chef"}},
    });
    let transport = DiscordTransport::new("0".repeat(64), "app-1".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].chat_title.as_deref(), Some("general"));
}

#[tokio::test]
async fn discord_dm_interaction_has_no_chat_title() {
    let payload = json!({
        "type": 2,
        "id": "interaction-2",
        "channel_id": "channel-dm",
        "data": {"name": "coach", "options": [{"name": "subcommand", "value": "Bonjour"}]},
        "user": {"id": "user-1", "username": "Chef"},
    });
    let transport = DiscordTransport::new("0".repeat(64), "app-1".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].chat_title.is_none());
}

// -------------------------------------------------------------------------
// Slack — Events API carries only the channel ID. Resolution is host-side.
// -------------------------------------------------------------------------

#[tokio::test]
async fn slack_event_has_no_chat_title() {
    let payload = json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "user": "U123",
            "text": "Hello",
            "channel": "C456",
            "channel_type": "channel",
            "ts": "1234567890.000100",
        }
    });
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].chat_title.is_none());
}

// -------------------------------------------------------------------------
// WhatsApp & Messenger — both deliver only 1:1 conversations to webhooks,
// so chat_title is structurally always None.
// -------------------------------------------------------------------------

#[tokio::test]
async fn whatsapp_dm_has_no_chat_title() {
    let payload = json!({
        "object": "whatsapp_business_account",
        "entry": [{"id": "biz-1", "changes": [{"value": {
            "messaging_product": "whatsapp",
            "metadata": {"display_phone_number": "1", "phone_number_id": "1"},
            "contacts": [{"wa_id": "5141234567", "profile": {"name": "Chef"}}],
            "messages": [{
                "from": "5141234567",
                "id": "wa-msg-1",
                "type": "text",
                "text": {"body": "Hello"},
                "timestamp": "1700000000",
            }]
        }, "field": "messages"}]}]
    });
    let transport = WhatsAppTransport::new("app-secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].chat_title.is_none());
}

#[tokio::test]
async fn messenger_dm_has_no_chat_title() {
    let payload = json!({
        "object": "page",
        "entry": [{"id": "page-1", "messaging": [{
            "sender": {"id": "user-1"},
            "recipient": {"id": "page-1"},
            "timestamp": 1_700_000_000_000_i64,
            "message": {"mid": "m_1", "text": "Hello"},
        }]}]
    });
    let transport = MessengerTransport::new("app-secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1);
    assert!(messages[0].chat_title.is_none());
}

/// An `m.me/{page}?ref={code}` deep link must surface its code.
///
/// This is how Messenger does account linking, and it arrives as a bare
/// `referral` when the person has never messaged the page. Until this was
/// parsed, the event was indistinguishable from someone saying hello, so the
/// code never reached the consumer and Messenger could not be linked at all.
#[tokio::test]
async fn messenger_referral_deep_link_surfaces_its_code() {
    let payload = json!({
        "object": "page",
        "entry": [{"messaging": [{
            "sender": {"id": "psid-1"},
            "recipient": {"id": "page-1"},
            "timestamp": 1_700_000_000_000_i64,
            "referral": {"ref": "LINKCODE123", "source": "SHORTLINK", "type": "OPEN_THREAD"},
        }]}]
    });
    let transport = MessengerTransport::new("app-secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));

    assert_eq!(messages.len(), 1, "a referral must produce one message");
    match &messages[0].content {
        MessageContent::Text { body } => assert_eq!(
            body, "LINKCODE123",
            "the ref must be surfaced as the body so existing link-code detection sees it"
        ),
        other => panic!("expected text content, got {other:?}"),
    }
    assert_eq!(messages[0].sender_id, "psid-1");
}

/// The same code also arrives nested under `postback.referral` when the person
/// lands on Get Started rather than straight into a thread. Both paths must
/// work or linking succeeds for some users and silently fails for others.
#[tokio::test]
async fn messenger_get_started_referral_also_surfaces_its_code() {
    let payload = json!({
        "object": "page",
        "entry": [{"messaging": [{
            "sender": {"id": "psid-2"},
            "recipient": {"id": "page-1"},
            "timestamp": 1_700_000_000_000_i64,
            "postback": {
                "mid": "m_2",
                "title": "Get Started",
                "payload": "GET_STARTED",
                "referral": {"ref": "LINKCODE456", "source": "SHORTLINK", "type": "OPEN_THREAD"},
            },
        }]}]
    });
    let transport = MessengerTransport::new("app-secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));

    assert_eq!(messages.len(), 1);
    match &messages[0].content {
        MessageContent::Text { body } => assert_eq!(
            body, "LINKCODE456",
            "the Get Started referral must surface the code, not the GET_STARTED payload"
        ),
        other => panic!("expected text content, got {other:?}"),
    }
}
