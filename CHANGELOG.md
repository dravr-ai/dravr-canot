# Changelog

## [0.4.28] — 2026-09-04



## [0.4.27] — 2026-09-02

### Added

- feat(rich_text): `parse_markdown`, `render_rich_text` and `escape_markdown` — a host that authors its strings in inline markdown hands the channels the dialect they already translate

### Fixed

- fix(render): a Card body is rendered through the channel's rich-text translator instead of being escaped as plain text, so a `<b>` heading is bold on every channel
- fix(commands): drop `CommandDefinition::response_template`, a field no host or channel ever read

## [0.4.25] — 2026-08-28



## [0.4.24] — 2026-08-26



## [0.4.22] — 2026-08-20



## [0.4.21] — 2026-08-18

### Fixed

- fix(telegram): acknowledge a tapped button so it does not look dead
- fix(deps): bump quinn-proto past the memory-exhaustion advisory
- fix: repair the SessionStart bootstrap guard for an empty .build



## [0.4.18] — 2026-06-19

### Changed

- deps: migrate `dravr-canot-mcp` and `dravr-canot-server` to dravr-tronc 0.5.3
  (dual-era MCP engine). tronc 0.5 hands tools a shared `&Arc<ServerState>` with
  no outer `RwLock`, so `ServerState.configs` moved to per-field interior
  mutability (`RwLock<HashMap<…>>`); `get_config`/`set_config` are now `&self`
  async.

### Fixed

- lint: rewrite an SSE-frame `loop`/`let-else` as `while let` in
  `agui_consumer` (rust-1.96 `clippy::while_let_loop`).

## [0.4.17] — 2026-06-02



## [0.4.16] — 2026-06-01

### Added

- feat(telegram): add delete_message to MessagingChannel for private slash echoes No-op default; Telegram override calls deleteMessage (needs bot admin rights).

### Other

- ci(release): inline build+release, skip artifact-storage roundtrip Bypasses private-repo artifact quota by attaching binaries directly to GH Release via softprops
- ci(release): cap upload-artifact retention at 7d



## [0.4.15] — 2026-05-16



## [0.4.14] — 2026-05-15



## [0.4.13] — 2026-05-07



## [0.4.12] — 2026-05-01

### Other

- obs: structured ingress logging in canot transports Info-level body_len + parse summary, trace-level full webhook body in Telegram + Messenger parse_inbound; Telegram emits turn_id at parse.



## [0.4.11] — 2026-04-28



## [0.4.10] — 2026-04-28



## [0.4.9] — 2026-04-24



## [0.4.8] — 2026-04-23



## [0.4.7] — 2026-04-21



## [0.4.6] — 2026-04-20



## [0.4.5] — 2026-04-20

### Added

- feat(agui): SSE consumer + StatusAdapter + Telegram implementation AgUiConsumer streams AG-UI events over SSE; StatusAdapter trait + Telegram impl render progress via editMessageText (1.1s client-side throttle, bypassable for tests); drive_status_updates docs clarify caller owns finalize lifecycle.



## [0.4.4] — 2026-04-12

### Added

- feat(telegram): add forum topic support via message_thread_id Extract thread_id from inbound messages, propagate through OutgoingMessage, render in sendMessage
- feat: add dravr-canot-claude-channel crate MCP stdio server bridging Slack/Telegram webhooks to Claude Code sessions



## [0.4.3] — 2026-04-10

### Other

- build: reduce tokio feature footprint to minimal set



## [0.4.2] — 2026-04-08



## [0.4.1] — 2026-04-08

### Fixed

- fix(discord): reset reconnect counter after successful gateway session — previously the counter never reset, causing the gateway to permanently die after accumulating disconnections over time
- fix(discord): exit immediately on ReceiverDropped instead of retrying

## [0.4.0] — 2026-04-08



## [0.3.4] — 2026-04-07

### Fixed

- fix(telegram): skip service messages (member added/removed) in group chats



## [0.3.3] — 2026-03-31

### Added

- feat: integrate dravr-build-config submodule (validation violations to fix)

### Fixed

- fix: resolve error handling violations found by dravr-build-config validation



## [0.3.2] — 2026-03-26

### Other

- deps: bump dravr-tronc to 0.2 with error notification support



## [0.3.1] — 2026-03-23



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
