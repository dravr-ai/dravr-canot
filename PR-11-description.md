## Implementation Plan & Architecture

### Overview

`dravr-canot-claude-channel` is a new binary crate that bridges messaging platforms (Slack, Telegram, etc.) to Claude Code sessions via the Claude Channels MCP protocol. Claude Code spawns it as an MCP subprocess over stdio; it simultaneously runs an HTTP webhook server to receive inbound platform messages.

### Architecture

```
Slack/Telegram ──webhook──▶ Axum HTTP server ──mpsc──▶ MCP stdio loop ──▶ Claude Code
                             (POST /webhook/:channel)    (JSON-RPC 2.0)
                                    │                          │
                              sender_gate               permission_relay
                          (allowlist filter)         (tool approval via chat)
```

### Crate Structure (6 files)

| File | Responsibility |
|------|---------------|
| `main.rs` | CLI entry point (`clap`). Loads channel configs from env vars, registers adapters, runs webhook server + MCP loop concurrently via `tokio::select!` |
| `mcp.rs` | MCP stdio handler. JSON-RPC 2.0 `initialize` (advertises `claude/channel` + `claude/channel/permission` capabilities), `tools/list`, `tools/call` for the `reply` tool. Emits `notifications/claude/channel` for inbound messages and `notifications/claude/channel/permission/verdict` for approval responses |
| `webhook.rs` | Axum `POST /webhook/{channel}` — verifies webhook signatures, parses messages via existing adapters, gates by sender allowlist, forwards to MCP loop via `mpsc` channel |
| `permission.rs` | Permission relay — tracks the last active sender, formats tool-approval prompts, parses `yes/no <5-char-id>` verdict replies from chat users |
| `sender_gate.rs` | CSV-based sender allowlist. Empty list = open mode (all senders accepted) |
| `scripts/tunnel.sh` | Helper script that auto-detects `cloudflared` or `ngrok` and exposes the local webhook port |

### Key Design Decisions

1. **Reuses existing core library** — `ChannelRegistry`, `EnvConfigStore`, `create_adapter_from_config`, and all channel adapters from `dravr-canot` are used directly. No duplication.
2. **Sender gating** — prevents prompt injection by only forwarding messages from an explicit allowlist (`--allowed-senders` / `DRAVR_CHANNEL_ALLOWED_SENDERS`). Open mode warns at startup.
3. **Permission relay** — Claude Code's tool approval prompts are forwarded to the last person who sent a message, and their `yes/no <id>` reply is parsed and relayed back. Uses the Claude Code 5-char ID alphabet (`[a-km-z]`, skips `l`).
4. **Async architecture** — `tokio::select!` runs the MCP stdio loop and Axum webhook server concurrently. Webhook → MCP forwarding uses a bounded `mpsc` channel (256 buffer).

### Configuration

| Env Var / CLI Flag | Purpose |
|---|---|
| `DRAVR_CHANNEL_WEBHOOK_PORT` / `--webhook-port` | Webhook listen port (default: 3001) |
| `DRAVR_CHANNEL_ALLOWED_SENDERS` / `--allowed-senders` | Comma-separated sender ID allowlist |
| `SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET` | Slack adapter (from core) |
| `TELEGRAM_BOT_TOKEN`, `TELEGRAM_WEBHOOK_SECRET` | Telegram adapter (from core) |

### Tests

23 unit tests covering:
- MCP protocol responses (initialize capabilities, tools list, channel notifications)
- Permission verdict parsing (yes/no, short forms, case insensitivity, ID validation, whitespace tolerance)
- Sender gate logic (CSV parsing, allowlist filtering, open mode)
- Webhook message content extraction (text, media, location, card variants)

### Validation

All checks pass: `cargo fmt`, `cargo clippy` (zero warnings), `validate.sh` (architectural validation), 23/23 tests green.
