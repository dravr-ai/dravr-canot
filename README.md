# Canot — Multi-Platform Messaging

[![crates.io](https://img.shields.io/crates/v/dravr-canot.svg)](https://crates.io/crates/dravr-canot)
[![docs.rs](https://docs.rs/dravr-canot/badge.svg)](https://docs.rs/dravr-canot)
[![CI](https://github.com/dravr-ai/dravr-canot/actions/workflows/ci.yml/badge.svg)](https://github.com/dravr-ai/dravr-canot/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE.md)

Standalone Rust library for unified bidirectional messaging across **WhatsApp**, **Slack**, **Discord**, **Telegram**, and **Messenger**.

Instead of integrating with each platform's API separately (different auth schemes, webhook formats, payload structures), **Canot** provides a single trait-based interface — `MessagingChannel` — that normalizes inbound webhooks into `IncomingMessage` and renders outbound `OutgoingMessage` into each platform's native format. Webhook signature verification, replay protection, and retry logic are built in.

## Table of Contents

- [Install](#install)
- [Supported Channels](#supported-channels)
- [Quick Start](#quick-start)
- [REST API Server](#rest-api-server-dravr-canot-server)
- [MCP Server](#mcp-server-dravr-canot-mcp)
- [Channel Config Store](#channel-config-store)
- [Environment Variables](#environment-variables)
- [Docker](#docker)
- [Architecture](#architecture)
- [License](#license)

## Install

### Homebrew (macOS / Linux) — recommended

```bash
brew tap dravr-ai/tap
brew install dravr-canot
```

This installs two binaries:

- **`dravr-canot-server`** — REST API + MCP server (start with env vars for your channels)
- **`dravr-canot-mcp`** — standalone MCP server for editor/agent integration

Once installed, set your channel credentials and start the server:

```bash
export SLACK_BOT_TOKEN="xoxb-..."
export SLACK_SIGNING_SECRET="..."
dravr-canot-server --port 3000
```

### Docker

```bash
docker pull ghcr.io/dravr-ai/dravr-canot:latest
docker run -p 3000:3000 \
  -e SLACK_BOT_TOKEN="xoxb-..." \
  -e SLACK_SIGNING_SECRET="..." \
  ghcr.io/dravr-ai/dravr-canot
```

### Cargo (library)

```toml
[dependencies]
dravr-canot = { version = "0.4", features = ["channel-slack", "channel-whatsapp"] }
```

## Supported Channels

| Channel | Feature Flag | Signature Verification | Outbound Format |
|---------|-------------|----------------------|-----------------|
| Slack | `channel-slack` | HMAC-SHA256 v0 | Block Kit JSON |
| WhatsApp | `channel-whatsapp` | HMAC-SHA256 (Meta) | Cloud API v22.0 |
| Messenger | `channel-messenger` | HMAC-SHA256 (Meta) | Graph API templates |
| Discord | `channel-discord` | Ed25519 | Embeds + components |
| Telegram | `channel-telegram` | Secret token header | Bot API + HTML |

All channels support: text, media (image/video/audio/document), location, and rich cards with action buttons.

Use `features = ["all-channels"]` to compile all five, or pick only what you need.

## Quick Start

Use the `MessagingChannel` trait programmatically:

```rust
use dravr_canot::factory::create_adapter_from_config;
use dravr_canot::models::{ChannelType, MessageContent, OutgoingMessage};
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), dravr_canot::error::MessagingError> {
    let config = json!({ "webhook_secret": "your-slack-signing-secret" });
    let adapter = create_adapter_from_config(ChannelType::Slack, &config)?;

    let msg = OutgoingMessage {
        channel_type: ChannelType::Slack,
        recipient_id: "C1234567890".into(),
        content: MessageContent::Text { body: "Hello from Canot!".into() },
        correlation_id: Uuid::new_v4(),
        reply_to: None,
        thread_id: None,
    };

    // Render to Slack Block Kit JSON
    let payload = adapter.render(&msg)?;
    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
    Ok(())
}
```

## REST API Server (`dravr-canot-server`)

A unified HTTP server for webhook ingress and outbound message sending with built-in MCP support. Reads channel credentials from environment variables at startup.

### Usage

```bash
# Start with channels auto-configured from env vars
dravr-canot-server --port 3000

# MCP-only mode via stdio (for editor/agent integration)
dravr-canot-server --transport stdio
```

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/api/messaging/webhook/:channel` | Receive inbound webhook (slack, whatsapp, telegram, discord, messenger) |
| `POST` | `/api/messaging/send` | Send an outbound message |
| `GET` | `/health` | Server health check |
| `POST` | `/mcp` | MCP Streamable HTTP (JSON-RPC 2.0) |

### Webhook Example

Point your Slack Events API URL to:

```
https://your-domain.com/api/messaging/webhook/slack
```

Canot verifies the signature, parses the payload into a normalized `IncomingMessage`, and returns it as JSON. Slack's mrkdwn auto-formatting (`<mailto:x|x>`, `<url|label>`) is automatically stripped.

### Authentication

Optional. Set `DRAVR_API_KEY` to require bearer token auth. When unset, all requests pass through (localhost dev mode).

```bash
DRAVR_API_KEY=my-secret dravr-canot-server
curl http://localhost:3000/health -H "Authorization: Bearer my-secret"
```

## MCP Server (`dravr-canot-mcp`)

Exposes messaging channels via the [Model Context Protocol](https://modelcontextprotocol.io/). Any MCP-compatible client (Claude Desktop, editors, custom agents) can send messages and manage channel configs.

### Usage

```bash
# Stdio transport (default — for editor/client integration)
dravr-canot-mcp

# HTTP transport
dravr-canot-mcp --transport http --port 3000
```

### MCP Tools

| Tool | Description |
|------|-------------|
| `list_channels` | List registered messaging channels and their status |
| `send_message` | Send a message to a channel (text, media, card) |
| `get_channel_config` | Get configuration for a channel type |
| `set_channel_config` | Set or update channel credentials |

### Client Configuration

Add to your MCP client config (e.g. Claude Desktop `claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "canot": {
      "command": "dravr-canot-mcp"
    }
  }
}
```

## Channel Config Store

Canot uses the `ChannelConfigStore` trait for pluggable configuration backends. This follows the same pattern as [embacle](https://github.com/dravr-ai/dravr-embacle)'s `LlmProvider` trait.

### Built-in: Environment Variables

`EnvConfigStore` reads channel credentials from env vars at startup:

```rust
use dravr_canot::EnvConfigStore;
use dravr_canot::config_store::ChannelConfigStore;

let store = EnvConfigStore::from_env();
let channels = store.list_configured_channels().await;
let config = store.get_config(ChannelType::Slack).await;
```

### Custom: Database Backend

Implement the trait for your storage layer:

```rust
use dravr_canot::config_store::ChannelConfigStore;
use dravr_canot::models::{ChannelConfig, ChannelType};

#[async_trait]
impl ChannelConfigStore for MyDatabase {
    async fn get_config(&self, channel_type: ChannelType) -> Option<ChannelConfig> {
        self.query_channel_config(channel_type).await
    }

    async fn get_tenant_config(&self, tenant_id: &str, channel_type: ChannelType) -> Option<ChannelConfig> {
        self.query_tenant_channel_config(tenant_id, channel_type).await
    }

    async fn list_configured_channels(&self) -> Vec<ChannelType> {
        self.list_active_channels().await
    }
}
```

## Environment Variables

### Channel Credentials

| Variable | Channel | Maps to |
|----------|---------|---------|
| `SLACK_BOT_TOKEN` | Slack | `api_key` |
| `SLACK_SIGNING_SECRET` | Slack | `webhook_secret` |
| `TELEGRAM_BOT_TOKEN` | Telegram | `bot_token` |
| `TELEGRAM_WEBHOOK_SECRET` | Telegram | `webhook_secret` |
| `META_WHATSAPP_ACCESS_TOKEN` | WhatsApp | `api_key` |
| `META_WHATSAPP_APP_SECRET` | WhatsApp | `webhook_secret` |
| `META_WHATSAPP_PHONE_NUMBER_ID` | WhatsApp | `phone_number` |
| `META_WHATSAPP_VERIFY_TOKEN` | WhatsApp | `verify_token` |
| `DISCORD_BOT_TOKEN` | Discord | `bot_token` |
| `DISCORD_PUBLIC_KEY` | Discord | `webhook_secret` |
| `DISCORD_APPLICATION_ID` | Discord | `account_id` |
| `MESSENGER_PAGE_ACCESS_TOKEN` | Messenger | `api_key` |
| `MESSENGER_APP_SECRET` | Messenger | `webhook_secret` |
| `MESSENGER_VERIFY_TOKEN` | Messenger | `verify_token` |

### Runtime Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DRAVR_MAX_RETRY_ATTEMPTS` | `3` | Max delivery retry attempts before dead-lettering |
| `DRAVR_LINK_CODE_TTL_MINUTES` | `10` | Link verification code expiration |
| `DRAVR_OTP_TTL_MINUTES` | `10` | OTP code expiration |
| `DRAVR_MAX_OTP_ATTEMPTS` | `3` | Max OTP verification attempts per flow |
| `DRAVR_MAX_OTP_FLOWS_PER_HOUR` | `5` | Rate limit: OTP flows per channel user per hour |
| `DRAVR_API_KEY` | *(none)* | Bearer token auth for server endpoints |

## AG-UI Progress Feedback (feature flag)

Enable the `agui` feature to render
[AG-UI protocol](https://github.com/ag-ui-protocol/ag-ui) events —
the canonical progress-feedback format produced by
[`dravr-platform`](https://github.com/dravr-ai/dravr-platform). Useful for
rendering "the assistant is thinking…" status that updates in place as
pipeline stages advance.

```toml
[dependencies]
dravr-canot = { version = "0.4", features = ["agui", "channel-telegram"] }
```

**Decoding events.** The platform hands each event to the host process
as JSON on the run's broadcast; `AgUiEvent` is the wire schema it decodes
into. Unknown event kinds route to `AgUiEvent::Unknown` so the decoder
tolerates forward-compat events added to the AG-UI spec after the library
is released.

```rust
use dravr_canot::agui_consumer::AgUiEvent;

let event: AgUiEvent = serde_json::from_str(payload)?;
if let AgUiEvent::StepStarted { step_name, .. } = &event {
    eprintln!("→ {step_name}");
}
```

**Telegram integration.** `TelegramStatusAdapter` sends an initial
`sendMessage` with "thinking…", then `editMessageText`es it in place as
each AG-UI event arrives so the chat history shows a single evolving
status message rather than a wall of placeholders.

```rust
use dravr_canot::agui_status::{status_text_for_event, StatusAdapter};
use dravr_canot::channels::telegram::agui_status::TelegramStatusAdapter;

let adapter = TelegramStatusAdapter::open(
    std::env::var("TELEGRAM_BOT_TOKEN")?,
    chat_id,
    /* message_thread_id */ None,
    "thinking…",
).await?;

while let Ok(payload) = events.recv().await {
    let event: AgUiEvent = serde_json::from_str(&payload)?;
    if let Some(text) = status_text_for_event(&event) {
        let _ = adapter.set_status(&text).await;
    }
}

adapter.finalize("Your last run was 5 km at 4:30/km.").await?;
```

The adapter implements the channel-agnostic [`StatusAdapter`] trait, so
Slack, Discord, and Messenger can follow the same pattern with their
own edit-in-place primitives.

## Docker

```bash
docker pull ghcr.io/dravr-ai/dravr-canot:latest
```

Pass channel credentials as environment variables:

```bash
docker run -p 3000:3000 \
  -e SLACK_BOT_TOKEN="xoxb-..." \
  -e SLACK_SIGNING_SECRET="..." \
  -e TELEGRAM_BOT_TOKEN="..." \
  -e TELEGRAM_WEBHOOK_SECRET="..." \
  ghcr.io/dravr-ai/dravr-canot
```

## Architecture

```
Your Application
    └── dravr-canot (this library)
            │
            ├── Channel Adapters (transport + renderer per platform)
            │   ├── SlackChannel        → HMAC-SHA256 v0, Block Kit, Events API
            │   ├── WhatsAppChannel     → HMAC-SHA256 (Meta), Cloud API v22.0
            │   ├── MessengerChannel    → HMAC-SHA256 (Meta), Graph API templates
            │   ├── DiscordChannel      → Ed25519, Embeds + components
            │   └── TelegramChannel     → Secret token, Bot API + HTML
            │
            ├── Core Traits
            │   ├── MessagingChannel    → verify_signature, receive, render, send, send_raw
            │   ├── TransportAdapter    → wire protocol: signature, parse, send_raw
            │   ├── ResponseRenderer    → format OutgoingMessage → channel-native payload
            │   ├── ChannelDescriptor   → metadata: name, webhook path, capabilities
            │   └── ChannelConfigStore  → pluggable config backend (env, database, custom)
            │
            ├── Infrastructure
            │   ├── ChannelRegistry     → route webhooks to the correct adapter
            │   ├── EnvConfigStore      → read channel credentials from env vars
            │   ├── MessagingConfig     → runtime settings from DRAVR_* env vars
            │   └── RetryWorker         → exponential backoff with dead-letter queue
            │
            ├── MCP Server (library + binary crate, powered by dravr-tronc)
            │   └── dravr-canot-mcp     → JSON-RPC 2.0 over stdio or HTTP/SSE
            │
            └── Unified REST API + MCP Server (binary crate, powered by dravr-tronc)
                └── dravr-canot-server  → webhook ingress, message sending, health, MCP
```

All channel adapters implement the same `MessagingChannel` trait:
- **`verify_signature()`** — cryptographic webhook verification (constant-time)
- **`receive()`** — parse inbound webhook into normalized `IncomingMessage`
- **`render()`** — format `OutgoingMessage` into channel-native payload
- **`send()`** — render + deliver outbound message
- **`send_raw()`** — deliver a pre-rendered payload (for retry queue)

For detailed API docs see [docs.rs/dravr-canot](https://docs.rs/dravr-canot).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
