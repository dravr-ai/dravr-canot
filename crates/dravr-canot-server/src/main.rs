// ABOUTME: CLI entry point for the unified dravr-canot server binary
// ABOUTME: Reads channel configs from env vars, registers adapters, serves REST + MCP
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use clap::Parser;
use dravr_canot::config_store::ChannelConfigStore;
use dravr_canot::factory::create_adapter_from_config;
use dravr_canot::models::ChannelType;
use dravr_canot::ChannelRegistry;
use dravr_canot::EnvConfigStore;
use dravr_canot_mcp::ServerState;
use dravr_tronc::server::cli::ServerArgs;
use tokio::sync::RwLock;
use tracing::{info, warn};

use dravr_canot_server::router;

/// dravr-canot-server — Unified REST API + MCP server for multi-platform messaging
#[derive(Parser)]
#[command(name = "dravr-canot-server", version, about)]
struct Cli {
    #[command(flatten)]
    server: ServerArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();
    dravr_tronc::server::tracing_init::init(&cli.server.transport);

    // Load channel configs from env vars and register adapters
    let env_store = EnvConfigStore::from_env();
    let configured_channels = env_store.list_configured_channels().await;
    let mut registry = ChannelRegistry::new();

    for channel_type in &configured_channels {
        if let Some(config) = env_store.get_config(*channel_type).await {
            let config_json = serde_json::to_value(&config).unwrap_or_default();
            match create_adapter_from_config(*channel_type, &config_json) {
                Ok(adapter) => {
                    registry.register(adapter);
                    info!(channel = %channel_type, "Registered channel adapter from env");
                }
                Err(e) => {
                    warn!(channel = %channel_type, error = %e, "Failed to register channel adapter");
                }
            }
        }
    }

    let state = Arc::new(RwLock::new(ServerState::new(registry)));

    // Seed configs into server state so webhook/send handlers can access them
    {
        let mut guard = state.write().await;
        for channel_type in &configured_channels {
            if let Some(config) = env_store.get_config(*channel_type).await {
                guard.set_config(*channel_type, config);
            }
        }
    }

    if configured_channels.is_empty() {
        info!("No channel env vars found — channels can be configured via MCP set_channel_config tool");
    } else {
        info!(
            channels = ?configured_channels.iter().map(ChannelType::to_string).collect::<Vec<_>>(),
            "Channels loaded from env vars"
        );
    }

    info!(
        transport = %cli.server.transport,
        "Starting dravr-canot server"
    );

    match cli.server.transport.as_str() {
        "stdio" => {
            let server = Arc::new(dravr_tronc::McpServer::new(
                "dravr-canot",
                env!("CARGO_PKG_VERSION"),
                dravr_canot_mcp::build_tool_registry(),
                Arc::clone(&state),
            ));
            dravr_tronc::mcp::transport::stdio::run(server).await?;
        }
        "http" => {
            let app = router::build(state);
            let addr = format!("{}:{}", cli.server.host, cli.server.port);
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| format!("Failed to bind {addr}: {e}"))?;

            info!(address = %addr, "HTTP transport listening");

            axum::serve(listener, app)
                .await
                .map_err(|e| format!("Server error: {e}"))?;
        }
        other => {
            return Err(format!("Unknown transport: {other}. Valid: http, stdio").into());
        }
    }

    Ok(())
}
