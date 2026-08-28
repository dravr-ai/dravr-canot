// ABOUTME: Asserts /command@thisbot is rewritten to /command before the matcher sees it
// ABOUTME: and that a command aimed at another bot in the same group is left untouched

// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Telegram appends `@<botusername>` to the command entity whenever a command
//! is tapped in a chat holding more than one bot, and that suffixed form is
//! the one guaranteed to reach a privacy-mode bot. The command matcher
//! requires a word boundary after the command key, so an unstripped suffix
//! turned every such invocation into an unknown-command reply — the bot heard
//! the command and answered "I don't know that".
//!
//! These tests assert the rewritten body by value, because a regression here
//! is invisible: `parse_inbound` still returns exactly one well-formed
//! message either way, so anything weaker than an equality check on the text
//! passes against the broken behaviour.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(feature = "channel-telegram")]

use dravr_canot::channels::telegram::transport::TelegramTransport;
use dravr_canot::models::MessageContent;
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

/// Bot identity shared by every test here. The username cache is
/// process-wide, so all tests seed the SAME pair and parallel execution
/// cannot make them interfere.
const BOT_ID: i64 = 999_002;
const BOT_USERNAME: &str = "dravr_fitness_bot";

fn transport() -> TelegramTransport {
    TelegramTransport::with_bot_identity("secret".to_owned(), BOT_ID, BOT_USERNAME)
}

fn group_text(text: &str) -> Value {
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

/// Parse one update and return the text body that reaches the dispatcher.
async fn parsed_body(update: &Value) -> String {
    let bytes = serde_json::to_vec(update).unwrap_or_else(|e| panic!("serialize: {e}"));
    let messages = transport()
        .parse_inbound(&HeaderMap::new(), &bytes)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1, "expected exactly one parsed message");
    match &messages[0].content {
        MessageContent::Text { body } => body.clone(),
        other => panic!("expected text content, got {other:?}"),
    }
}

#[tokio::test]
async fn own_suffix_is_stripped() {
    assert_eq!(
        parsed_body(&group_text("/help@dravr_fitness_bot")).await,
        "/help"
    );
}

#[tokio::test]
async fn own_suffix_is_stripped_case_insensitively() {
    // Telegram preserves the username's registered casing, but clients and
    // humans both produce variants; the match must not depend on it.
    assert_eq!(
        parsed_body(&group_text("/help@DrAvR_FiTnEsS_BoT")).await,
        "/help"
    );
}

#[tokio::test]
async fn arguments_survive_the_strip() {
    assert_eq!(
        parsed_body(&group_text("/group@dravr_fitness_bot invite alice")).await,
        "/group invite alice"
    );
}

#[tokio::test]
async fn another_bots_command_is_left_untouched() {
    // The guard that matters: a blanket strip would make Dravr answer a
    // command explicitly aimed at a different bot in a shared group. Without
    // this test the strip passes vacuously.
    assert_eq!(
        parsed_body(&group_text("/help@some_other_bot")).await,
        "/help@some_other_bot"
    );
}

#[tokio::test]
async fn a_prefix_of_our_username_is_not_our_bot() {
    // `dravr_fitness` is a prefix of `dravr_fitness_bot`; an inequality test
    // that compared prefixes rather than the whole suffix would strip this.
    assert_eq!(
        parsed_body(&group_text("/help@dravr_fitness")).await,
        "/help@dravr_fitness"
    );
}

#[tokio::test]
async fn bare_command_is_unchanged() {
    assert_eq!(parsed_body(&group_text("/help")).await, "/help");
}

#[tokio::test]
async fn prose_mentioning_the_bot_is_not_rewritten() {
    // Only the command entity carries the suffix. Ordinary text that happens
    // to mention the bot must reach the coach verbatim.
    let text = "ask @dravr_fitness_bot about my run";
    assert_eq!(parsed_body(&group_text(text)).await, text);
}

#[tokio::test]
async fn suffix_on_a_later_token_is_not_rewritten() {
    // Telegram marks only the first token as the bot_command entity, so an
    // `@` deeper in the line is user content, not addressing.
    let text = "/group invite @dravr_fitness_bot";
    assert_eq!(parsed_body(&group_text(text)).await, text);
}

#[tokio::test]
async fn stripping_does_not_cost_the_addressing_signal() {
    // `/command@thisbot` IS the mention that addresses the bot in a group.
    // The rewrite happens on the content only; addressing is computed from
    // the raw payload, so a stripped command must still read as addressed.
    let bytes = serde_json::to_vec(&group_text("/help@dravr_fitness_bot"))
        .unwrap_or_else(|e| panic!("{e}"));
    let messages = transport()
        .parse_inbound(&HeaderMap::new(), &bytes)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert!(
        messages[0].addressed_to_bot,
        "a /command@thisbot in a group must still count as addressed to the bot"
    );
}
