// ABOUTME: Tests for SlackTransport bot-id allow-list behaviour
// ABOUTME: Verifies bot messages are dropped by default but pass through when allow-listed
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(feature = "channel-slack")]

use dravr_canot::channels::slack::transport::SlackTransport;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

fn to_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_else(|e| panic!("serialize payload: {e}"))
}

fn slack_bot_event(bot_id: &str, text: &str) -> Value {
    json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "user": "U999",
            "bot_id": bot_id,
            "text": text,
            "channel": "C123",
            "channel_type": "channel",
            "ts": "1700000000.000200",
        }
    })
}

fn slack_human_event(text: &str) -> Value {
    json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "user": "U123",
            "text": text,
            "channel": "C123",
            "channel_type": "channel",
            "ts": "1700000000.000300",
        }
    })
}

#[tokio::test]
async fn bot_message_dropped_by_default() {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_bot_event("B_UNLISTED", "hi from bot")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(
        messages.is_empty(),
        "unlisted bot message must be dropped; got {messages:?}"
    );
}

#[tokio::test]
async fn allowed_bot_message_passes_through() {
    let transport =
        SlackTransport::with_allowed_bot_ids("secret".to_owned(), vec!["B_QA_DRIVER".to_owned()]);
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_bot_event("B_QA_DRIVER", "scope-refusal probe")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(
        messages.len(),
        1,
        "allow-listed bot message must parse to one IncomingMessage"
    );
    assert_eq!(messages[0].channel_message_id, "1700000000.000200");
}

#[tokio::test]
async fn non_allowed_bot_still_dropped_when_list_is_non_empty() {
    let transport =
        SlackTransport::with_allowed_bot_ids("secret".to_owned(), vec!["B_QA_DRIVER".to_owned()]);
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_bot_event("B_UNRELATED", "noise")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(
        messages.is_empty(),
        "non-matching bot_id must still be dropped"
    );
}

#[tokio::test]
async fn human_message_always_passes_regardless_of_list() {
    for list in [vec![], vec!["B_QA_DRIVER".to_owned()]] {
        let transport = SlackTransport::with_allowed_bot_ids("secret".to_owned(), list.clone());
        let messages = transport
            .parse_inbound(&HeaderMap::new(), &to_bytes(&slack_human_event("hello")))
            .await
            .unwrap_or_else(|e| panic!("parse_inbound (list={list:?}): {e}"));
        assert_eq!(
            messages.len(),
            1,
            "human message must parse through with allow-list {list:?}"
        );
    }
}
