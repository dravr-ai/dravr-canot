# Changelog

## [0.1.1] — 2026-03-18



## [0.1.0] — 2026-03-18



## [0.1.0] — 2026-03-18

### Added

- Core `MessagingChannel`, `TransportAdapter`, and `ResponseRenderer` traits
- Channel adapters: WhatsApp, Slack, Discord, Telegram, Messenger
- Webhook signature verification (HMAC-SHA256, Ed25519, secret token)
- `ChannelRegistry` for routing inbound webhooks
- `create_adapter_from_config` factory for on-demand adapter construction
- Outbound retry worker with exponential backoff and dead-letter queue
- `dravr-canot-mcp` — MCP server with stdio and HTTP/SSE transports
- `dravr-canot-server` — unified REST API + MCP server binary
- Docker image published to `ghcr.io/dravr-ai/dravr-canot`
- Homebrew formula via `brew tap dravr-ai/tap`
