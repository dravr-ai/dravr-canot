// ABOUTME: CLI entry point for the Claude Code channel server binary
// ABOUTME: Loads env config, registers adapters, runs MCP stdio + webhook server concurrently
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::error::Error as StdError;
use std::io;
use std::sync::Arc;

use clap::Parser;
use dravr_canot::config_store::ChannelConfigStore;
use dravr_canot::factory::create_adapter_from_config;
use dravr_canot::models::ChannelType;
use dravr_canot::ChannelRegistry;
use dravr_canot::EnvConfigStore;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::fmt as tracing_fmt;
use tracing_subscriber::EnvFilter;

mod mcp;
mod permission;
mod sender_gate;
mod webhook;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod test_support;

/// Channel event buffer size for the mpsc channel between webhook server and MCP loop
const EVENT_BUFFER_SIZE: usize = 256;

/// dravr-canot-claude-channel — Bridge Slack/Telegram webhooks to Claude Code sessions via MCP
#[derive(Parser)]
#[command(name = "dravr-canot-claude-channel", version, about)]
struct Cli {
    /// Port for the webhook HTTP server
    #[arg(long, default_value = "3001", env = "DRAVR_CHANNEL_WEBHOOK_PORT")]
    webhook_port: u16,

    /// Comma-separated list of allowed sender IDs (empty = allow all)
    #[arg(long, default_value = "", env = "DRAVR_CHANNEL_ALLOWED_SENDERS")]
    allowed_senders: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError + Send + Sync>> {
    // Initialize tracing (respects RUST_LOG env var)
    tracing_fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    // Load channel configs from env vars and register adapters
    let env_store = EnvConfigStore::from_env();
    let configured_channels = env_store.list_configured_channels().await;
    let mut registry = ChannelRegistry::new();
    let mut configs = HashMap::new();

    for channel_type in &configured_channels {
        if let Some(config) = env_store.get_config(*channel_type).await {
            let config_json = serde_json::to_value(&config).unwrap_or_default();
            match create_adapter_from_config(*channel_type, &config_json) {
                Ok(adapter) => {
                    registry.register(adapter);
                    configs.insert(*channel_type, config);
                    info!(channel = %channel_type, "Registered channel adapter");
                }
                Err(e) => {
                    warn!(channel = %channel_type, error = %e, "Failed to create adapter");
                }
            }
        }
    }

    if configured_channels.is_empty() {
        info!("No channel env vars found — set SLACK_BOT_TOKEN/SLACK_SIGNING_SECRET or TELEGRAM_BOT_TOKEN/TELEGRAM_WEBHOOK_SECRET");
    } else {
        info!(
            channels = ?configured_channels.iter().map(ChannelType::to_string).collect::<Vec<_>>(),
            "Channels loaded from env vars"
        );
    }

    let registry = Arc::new(registry);
    let configs = Arc::new(configs);
    let sender_gate = Arc::new(sender_gate::SenderGate::from_csv(&cli.allowed_senders));
    let permission_relay = Arc::new(permission::PermissionRelay::build()?);

    if sender_gate.is_empty() {
        warn!("No --allowed-senders specified — all senders will be accepted (open mode)");
    } else {
        info!(count = sender_gate.len(), "Sender allowlist configured");
    }

    // Create mpsc channel for webhook → MCP event forwarding
    let (event_tx, event_rx) = mpsc::channel(EVENT_BUFFER_SIZE);

    // Build webhook HTTP server
    let webhook_state = Arc::new(webhook::WebhookState {
        registry: Arc::clone(&registry),
        sender_gate,
        event_tx,
    });
    let app = webhook::build_router(webhook_state);
    let addr = format!("127.0.0.1:{}", cli.webhook_port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind webhook server to {addr}: {e}"))?;

    info!(address = %addr, "Webhook server listening");

    // Run MCP stdio loop and webhook server concurrently
    tokio::select! {
        result = mcp::run(event_rx, Arc::clone(&registry), Arc::clone(&configs), Arc::clone(&permission_relay)) => {
            if let Err(e) = result {
                error!(error = %e, "MCP server error");
            }
            info!("MCP server exited, shutting down");
        }
        result = axum::serve(listener, app) => {
            if let Err(e) = result {
                error!(error = %e, "Webhook server error");
            }
            info!("Webhook server exited");
        }
    }

    Ok(())
}
