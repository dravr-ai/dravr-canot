# Changelog

## [0.2.0] — 2026-03-18

### Added

- `ChannelConfigStore` trait for pluggable channel configuration backends (database, env vars, custom)
- `EnvConfigStore` reading `SLACK_*`, `TELEGRAM_*`, `META_WHATSAPP_*`, `DISCORD_*`, `MESSENGER_*` env vars
- Server auto-registers channel adapters from env vars at startup
- `strip_slack_formatting()` strips mrkdwn auto-formatting from inbound Slack messages

### Changed

- `ChannelConfig` fields `id`, `tenant_id`, `channel_type` now required (non-optional)
- `MessagingError` expanded to 17 variants (linking, session, OTP, rate limit, media)
- New models: `MessagingSession`, `LinkingMethod`, `MessagingLinkState`, `MessagingChannelLink`, `OutboundQueueEntry`
- All numeric constants configurable via `DRAVR_*` env vars through `MessagingConfig::from_env()`

## [0.1.2] — 2026-03-18

### Fixed

- Strip Slack mrkdwn formatting from inbound message text (fixes OTP email lookup)

## [0.1.1] — 2026-03-18

### Changed

- Align types with dravr-platform: required ChannelConfig fields, 17 error variants, session/linking models
- Add env-configurable messaging settings via `DRAVR_*` env vars

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
