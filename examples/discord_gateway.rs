// ABOUTME: Minimal Discord Gateway test — connects, prints all received messages
// ABOUTME: Usage: DISCORD_BOT_TOKEN=xxx cargo run --example discord_gateway --features channel-discord

use dravr_canot::channels::discord::gateway::{start_gateway, GatewayConfig};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "debug");
    tracing_subscriber::fmt::init();

    let token = std::env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN must be set");

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
