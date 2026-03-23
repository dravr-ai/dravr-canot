// ABOUTME: CLI entry point for the standalone dravr-canot MCP server binary
// ABOUTME: Supports stdio and HTTP transports for MCP JSON-RPC communication
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use clap::Parser;
use dravr_canot::ChannelRegistry;
use dravr_canot_mcp::ServerState;
use dravr_tronc::mcp::server::McpServer;
use dravr_tronc::server::cli::McpArgs;
use tokio::sync::RwLock;

/// dravr-canot-mcp — MCP server exposing messaging channels via Model Context Protocol
#[derive(Parser)]
#[command(name = "dravr-canot-mcp", version, about)]
struct Cli {
    #[command(flatten)]
    server: McpArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();
    dravr_tronc::server::tracing_init::init(&cli.server.transport);

    let registry = ChannelRegistry::new();
    let state = Arc::new(RwLock::new(ServerState::new(registry)));
    let tool_registry = dravr_canot_mcp::build_tool_registry();
    let server = Arc::new(McpServer::new(
        "dravr-canot-mcp",
        env!("CARGO_PKG_VERSION"),
        tool_registry,
        state,
    ));

    tracing::info!(
        transport = %cli.server.transport,
        "Starting dravr-canot MCP server"
    );

    match cli.server.transport.as_str() {
        "stdio" => dravr_tronc::mcp::transport::stdio::run(server).await?,
        "http" => {
            dravr_tronc::mcp::transport::http::serve(server, &cli.server.host, cli.server.port)
                .await?;
        }
        other => {
            return Err(format!("Unknown transport: {other}. Valid: stdio, http").into());
        }
    }

    Ok(())
}
