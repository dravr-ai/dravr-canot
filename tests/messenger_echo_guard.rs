// ABOUTME: Tests that Messenger drops the page's own echoed messages on inbound
// ABOUTME: Mirrors the Slack bot-id and Discord author.bot loop guards
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(feature = "channel-messenger")]

use dravr_canot::channels::messenger::transport::MessengerTransport;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

fn to_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_else(|e| panic!("serialize payload: {e}"))
}

/// One `messaging` event carrying a text message, optionally flagged as the
/// page's own echo.
fn messenger_event(text: &str, is_echo: bool) -> Value {
    let mut message = json!({
        "mid": "m_abc123",
        "text": text,
    });
    if is_echo {
        message["is_echo"] = json!(true);
        message["app_id"] = json!(1_234_567_890_u64);
    }
    json!({
        "object": "page",
        "entry": [{
            "id": "PAGE_ID",
            "time": 1_700_000_000_u64,
            "messaging": [{
                "sender": { "id": "USER_ID" },
                "recipient": { "id": "PAGE_ID" },
                "timestamp": 1_700_000_000_u64,
                "message": message,
            }]
        }]
    })
}

#[tokio::test]
async fn echoed_page_message_is_not_parsed_as_inbound() {
    let transport = MessengerTransport::new("app-secret".to_owned());
    let payload = messenger_event("the coach's own reply", true);

    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap();

    assert!(
        messages.is_empty(),
        "an is_echo message is the page's own outbound reply and must not re-enter \
         the pipeline, or the bot answers itself: {messages:?}"
    );
}

#[tokio::test]
async fn a_real_user_message_still_parses() {
    let transport = MessengerTransport::new("app-secret".to_owned());
    let payload = messenger_event("how was my run", false);

    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap();

    assert_eq!(messages.len(), 1, "one user message expected");
    assert_eq!(messages[0].sender_id, "USER_ID");
    assert_eq!(messages[0].channel_message_id, "m_abc123");
    assert!(messages[0].is_direct_message);
    assert!(messages[0].addressed_to_bot);
}

#[tokio::test]
async fn a_batch_keeps_the_user_message_and_drops_the_echo() {
    let transport = MessengerTransport::new("app-secret".to_owned());
    let payload = json!({
        "object": "page",
        "entry": [{
            "id": "PAGE_ID",
            "time": 1_700_000_000_u64,
            "messaging": [
                {
                    "sender": { "id": "USER_ID" },
                    "recipient": { "id": "PAGE_ID" },
                    "message": { "mid": "m_user", "text": "how was my run" },
                },
                {
                    "sender": { "id": "PAGE_ID" },
                    "recipient": { "id": "USER_ID" },
                    "message": { "mid": "m_echo", "text": "your run looked strong", "is_echo": true },
                }
            ]
        }]
    });

    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap();

    assert_eq!(messages.len(), 1, "only the user message survives");
    assert_eq!(messages[0].channel_message_id, "m_user");
}
