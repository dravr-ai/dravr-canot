// ABOUTME: A tapped inline button must be acknowledged, or Telegram shows the user nothing
// ABOUTME: Guards the coach-picker symptom — selection landed server-side while the tap looked dead

// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Telegram keeps a progress indicator on a tapped inline-keyboard button until
//! the bot calls `answerCallbackQuery`, and shows nothing at all if the call
//! never comes. The failure is entirely invisible server-side: the callback is
//! parsed, the command runs, the selection is written — and the athlete sees a
//! dead button and concludes the feature is broken. That is exactly how the
//! coach picker presented.
//!
//! What is covered here is that acknowledging never costs the selection: a
//! transport with no bot token, or a callback with no id, must still parse the
//! tap into a message. The outbound HTTP call itself is not asserted — canot has
//! no HTTP mock and the Telegram base URL is a hardcoded constant, so covering
//! it would mean adding both a mock dependency and a configurable base URL.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dravr_canot::channels::telegram::transport::TelegramTransport;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::json;

/// A button tap, as Telegram delivers it.
fn callback_update(with_id: bool) -> Vec<u8> {
    let mut callback = json!({
        "data": "/coach select 8f14e45f-ceea-467a-9ae2-d0a6f0e0b1c3",
        "from": {"id": 456, "first_name": "Chef"},
        "message": {
            "message_id": 42,
            "chat": {"id": 456, "type": "private"}
        }
    });
    if with_id {
        callback["id"] = json!("cbq-1");
    }
    serde_json::to_vec(&json!({ "update_id": 7, "callback_query": callback }))
        .expect("update serialises")
}

#[tokio::test]
async fn a_tap_still_becomes_a_message_without_a_bot_token() {
    // No token means the acknowledgement cannot be sent. The tap must survive
    // that: losing a spinner is acceptable, losing the coach selection is not.
    let transport = TelegramTransport::new("secret".to_owned());

    let messages = transport
        .parse_inbound(&HeaderMap::new(), &callback_update(true))
        .await
        .expect("callback parses");

    assert_eq!(
        messages.len(),
        1,
        "the tap must produce exactly one message"
    );
    let body = format!("{:?}", messages[0]);
    assert!(
        body.contains("/coach select"),
        "the command the button carries must reach the pipeline, got: {body}"
    );
}

#[tokio::test]
async fn a_callback_with_no_id_is_still_parsed() {
    // Telegram always sends an id, so this is defence against a malformed or
    // replayed update rather than a real shape. It must degrade to "no
    // acknowledgement", never to "no selection".
    let transport = TelegramTransport::new("secret".to_owned());

    let messages = transport
        .parse_inbound(&HeaderMap::new(), &callback_update(false))
        .await
        .expect("callback parses");

    assert_eq!(
        messages.len(),
        1,
        "a tap with no callback id must still select"
    );
}
