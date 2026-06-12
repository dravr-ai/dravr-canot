// ABOUTME: Minimal Discord Gateway test — connects, prints all received messages
// ABOUTME: Usage: DISCORD_BOT_TOKEN=xxx cargo run --example discord_gateway --features channel-discord
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]

use std::env;

use dravr_canot::channels::discord::gateway::{start_gateway, GatewayConfig};
use tokio::sync::mpsc;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() {
    env::set_var("RUST_LOG", "debug");
    fmt::init();

    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN must be set");

    println!("Starting Discord Gateway...");

    let config = GatewayConfig::new(token);
    let (tx, mut rx) = mpsc::channel(64);

    tokio::spawn(async move {
        start_gateway(config, tx).await;
    });

    while let Some(msg) = rx.recv().await {
        println!("=== MESSAGE RECEIVED ===");
        println!(
            "  Sender: {} ({})",
            msg.sender_name.as_deref().unwrap_or("?"),
            msg.sender_id
        );
        println!("  Channel: {:?}", msg.conversation_id);
        println!("  Content: {:?}", msg.content);
        println!("========================");
    }
}
