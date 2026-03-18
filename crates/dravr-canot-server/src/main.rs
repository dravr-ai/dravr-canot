// ABOUTME: CLI entry point for the unified dravr-canot server binary
// ABOUTME: Serves REST API + MCP over HTTP or stdio for multi-platform messaging
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use clap::Parser;
use dravr_canot::ChannelRegistry;
use dravr_canot_mcp::transport::McpTransport;
use dravr_canot_mcp::ServerState;
use tokio::sync::RwLock;

use dravr_canot_server::router;

/// dravr-canot-server — Unified REST API + MCP server for multi-platform messaging
#[derive(Parser)]
#[command(name = "dravr-canot-server", version, about)]
struct Cli {
    /// Transport mode: "http" for REST API + MCP, "stdio" for MCP-only stdin/stdout
    #[arg(long, default_value = "http")]
    transport: String,

    /// HTTP listen port (only used with --transport http)
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// HTTP listen host (only used with --transport http)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    // stdio transport needs stderr-only logging to keep stdout clean for JSON-RPC
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

    tracing::info!(
        transport = %cli.transport,
        "Starting dravr-canot server"
    );

    match cli.transport.as_str() {
        "stdio" => {
            let server = Arc::new(dravr_canot_mcp::McpServer::new(
                Arc::clone(&state),
                dravr_canot_mcp::build_tool_registry(),
            ));
            dravr_canot_mcp::transport::stdio::StdioTransport
                .serve(server)
                .await?;
        }
        "http" => {
            let app = router::build(state);
            let addr = format!("{}:{}", cli.host, cli.port);
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| format!("Failed to bind {addr}: {e}"))?;

            tracing::info!(address = %addr, "HTTP transport listening");

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
