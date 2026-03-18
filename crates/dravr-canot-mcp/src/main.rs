// ABOUTME: CLI entry point for the standalone dravr-canot MCP server binary
// ABOUTME: Supports stdio and HTTP transports for MCP JSON-RPC communication
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use clap::Parser;
use dravr_canot::ChannelRegistry;
use dravr_canot_mcp::transport::McpTransport;
use dravr_canot_mcp::{McpServer, ServerState};
use tokio::sync::RwLock;

/// dravr-canot-mcp — MCP server exposing messaging channels via Model Context Protocol
#[derive(Parser)]
#[command(name = "dravr-canot-mcp", version, about)]
struct Cli {
    /// Transport mode: "stdio" for stdin/stdout or "http" for HTTP+SSE
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// HTTP listen port (only used with --transport http)
    #[arg(long, default_value_t = 3001)]
    port: u16,

    /// HTTP listen host (only used with --transport http)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    let is_stdio = cli.transport == "stdio";
    if is_stdio {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    let registry = ChannelRegistry::new();
    let state = Arc::new(RwLock::new(ServerState::new(registry)));
    let tool_registry = dravr_canot_mcp::build_tool_registry();
    let server = Arc::new(McpServer::new(state, tool_registry));

    tracing::info!(
        transport = %cli.transport,
        "Starting dravr-canot MCP server"
    );

    match cli.transport.as_str() {
        "stdio" => {
            dravr_canot_mcp::transport::stdio::StdioTransport
                .serve(server)
                .await?;
        }
        "http" => {
            dravr_canot_mcp::transport::http::HttpTransport::new(cli.host, cli.port)
                .serve(server)
                .await?;
        }
        other => {
            return Err(format!("Unknown transport: {other}. Valid: stdio, http").into());
        }
    }

    Ok(())
}
