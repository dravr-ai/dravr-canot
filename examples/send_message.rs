// ABOUTME: Example showing how to create a channel adapter and render an outbound message
// ABOUTME: Demonstrates the factory pattern, message construction, and rendering pipeline
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai
//
// Run with: cargo run --example send_message --features channel-slack

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]

use dravr_canot::factory::create_adapter_from_config;
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::turn::ConversationTurnId;
use serde_json::json;

fn main() {
    // Create a Slack adapter from a JSON config
    let config = json!({
        "webhook_secret": "your-slack-signing-secret"
    });
    let adapter = create_adapter_from_config(ChannelType::Slack, &config)
        .expect("failed to create Slack adapter");

    println!("Created adapter for: {}", adapter.channel_type());

    // Build an outgoing message
    let msg = OutgoingMessage {
        channel_type: ChannelType::Slack,
        recipient_id: "C1234567890".into(),
        content: MessageContent::Text {
            body: "Hello from dravr-canot!".into(),
        },
        turn_id: ConversationTurnId::new(),
        reply_to: None,
        thread_id: None,
    };

    // Render to channel-specific payload (Slack Block Kit JSON)
    let payload = adapter.render(&msg).expect("failed to render message");
    println!(
        "Rendered payload:\n{}",
        serde_json::to_string_pretty(&payload).unwrap()
    );
}
