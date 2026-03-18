# dravr-canot

Unified multi-platform messaging library for **WhatsApp**, **Slack**, **Discord**, **Telegram**, and **Messenger**.

[![Crates.io](https://img.shields.io/crates/v/dravr-canot.svg)](https://crates.io/crates/dravr-canot)
[![docs.rs](https://docs.rs/dravr-canot/badge.svg)](https://docs.rs/dravr-canot)
[![CI](https://github.com/dravr-ai/dravr-canot/actions/workflows/ci.yml/badge.svg)](https://github.com/dravr-ai/dravr-canot/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/dravr-canot.svg)](LICENSE.md)

## Overview

dravr-canot provides a trait-based architecture for routing conversations through external chat platforms. Each channel implements two core traits:

- **`TransportAdapter`** — webhook signature verification, payload parsing, raw HTTP send
- **`ResponseRenderer`** — format `OutgoingMessage` into channel-specific payloads (Block Kit, Embeds, etc.)

## Integration Modes

1. **Programmatic** — use the `MessagingChannel` trait directly in your Rust code
2. **REST Server** — run `dravr-canot-server` as a standalone API server with webhook endpoints
3. **MCP Server** — run `dravr-canot-mcp` to expose channels via Model Context Protocol (stdio or HTTP/SSE)

## Install

### Homebrew

```sh
brew tap dravr-ai/tap
brew install dravr-canot
```

### Cargo

```sh
cargo install dravr-canot-server dravr-canot-mcp
```

### Docker

```sh
docker pull ghcr.io/dravr-ai/dravr-canot:latest
docker run -p 3000:3000 ghcr.io/dravr-ai/dravr-canot
```

## Library Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
dravr-canot = { version = "0.1", features = ["channel-slack", "channel-whatsapp"] }
```

### Feature Flags

| Feature | Channel |
|---------|---------|
| `channel-whatsapp` | WhatsApp Business Cloud API |
| `channel-messenger` | Meta Messenger Platform |
| `channel-discord` | Discord Bot API |
| `channel-slack` | Slack Events API |
| `channel-telegram` | Telegram Bot API |
| `all-channels` | All of the above |

### Example

```rust
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::factory::create_adapter_from_config;
use serde_json::json;
use uuid::Uuid;

let config = json!({ "webhook_secret": "your-signing-secret" });
let adapter = create_adapter_from_config(ChannelType::Slack, &config).unwrap();

let msg = OutgoingMessage {
    channel_type: ChannelType::Slack,
    recipient_id: "C1234567890".into(),
    content: MessageContent::Text { body: "Hello from dravr-canot!".into() },
    correlation_id: Uuid::new_v4(),
    reply_to: None,
};

// Render to Slack Block Kit JSON
let payload = adapter.render(&msg).unwrap();
```

## Architecture

```
dravr-canot (core library)
├── src/channel.rs          — MessagingChannel trait
├── src/transport.rs        — TransportAdapter trait
├── src/renderer.rs         — ResponseRenderer trait
├── src/channels/           — Per-channel implementations
│   ├── slack/
│   ├── discord/
│   ├── whatsapp/
│   ├── messenger/
│   └── telegram/
├── src/registry.rs         — ChannelRegistry for routing
└── src/factory.rs          — Adapter factory from JSON config

dravr-canot-mcp          — MCP server (stdio + HTTP/SSE)
dravr-canot-server       — REST API + MCP unified server
```

## Workspace Crates

| Crate | Description |
|-------|-------------|
| `dravr-canot` | Core library with traits and channel adapters |
| `dravr-canot-mcp` | MCP server exposing channels via Model Context Protocol |
| `dravr-canot-server` | Unified REST API + MCP server binary |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
