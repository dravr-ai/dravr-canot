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

/// A verbatim `message_changed` envelope as Slack delivers it.
///
/// The author fields live under `message`, never at the top level — that shape
/// is the whole point of the regression below.
fn slack_message_changed_event(bot_id: &str, text: &str) -> Value {
    json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "subtype": "message_changed",
            "channel": "C123",
            "channel_type": "channel",
            "ts": "1700000000.000500",
            "message": {
                "type": "message",
                "user": "U999",
                "bot_id": bot_id,
                "app_id": "A0AMJFX0PAQ",
                "text": text,
                "ts": "1700000000.000400",
                "edited": { "user": "U999", "ts": "1700000000.000500" },
            },
        }
    })
}

fn slack_message_deleted_event() -> Value {
    json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "subtype": "message_deleted",
            "channel": "C123",
            "channel_type": "channel",
            "ts": "1700000000.000700",
            "deleted_ts": "1700000000.000400",
            "previous_message": {
                "type": "message",
                "user": "U999",
                "bot_id": "B_COACH",
                "text": "gone",
                "ts": "1700000000.000400",
            },
        }
    })
}

/// AG-UI status streaming edits the coach's own reply through `chat.update`,
/// and Slack echoes each edit back as `message_changed`. Because Slack nests
/// the author under `message`, the envelope carries no top-level `bot_id` and
/// no top-level `user`: the bot-loop guard matched nothing and the edit parsed
/// as a message from sender `"unknown"`. Observed live on 2026-08-20 — every
/// coach turn in #qa-automation started an account-linking OTP flow against the
/// coach's own status update, burying the real reply.
#[tokio::test]
async fn coach_own_edit_is_not_ingested_as_a_new_message() {
    for list in [vec![], vec!["B_QA_DRIVER".to_owned()]] {
        let transport = SlackTransport::with_allowed_bot_ids("secret".to_owned(), list.clone());
        let messages = transport
            .parse_inbound(
                &HeaderMap::new(),
                &to_bytes(&slack_message_changed_event("B_COACH", "status update")),
            )
            .await
            .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
        assert!(
            messages.is_empty(),
            "message_changed must never parse into user input (list={list:?}); got {messages:?}"
        );
    }
}

/// An allow-listed bot editing a message is still an edit, not new input —
/// otherwise the QA driver's own edits would replay as fresh questions.
#[tokio::test]
async fn allowed_bot_edit_is_still_dropped() {
    let transport =
        SlackTransport::with_allowed_bot_ids("secret".to_owned(), vec!["B_QA_DRIVER".to_owned()]);
    let messages = transport
        .parse_inbound(
            &HeaderMap::new(),
            &to_bytes(&slack_message_changed_event("B_QA_DRIVER", "edited probe")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(
        messages.is_empty(),
        "an allow-listed bot's edit is still an edit; got {messages:?}"
    );
}

#[tokio::test]
async fn message_deleted_is_not_ingested() {
    let transport = SlackTransport::new("secret".to_owned());
    let messages = transport
        .parse_inbound(&HeaderMap::new(), &to_bytes(&slack_message_deleted_event()))
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(
        messages.is_empty(),
        "message_deleted is a tombstone, not user input; got {messages:?}"
    );
}
