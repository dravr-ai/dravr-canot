// ABOUTME: Unified multi-platform messaging library for WhatsApp, Slack, Discord, Telegram, Messenger
// ABOUTME: Trait-based architecture with transport adapters and response renderers per channel
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![deny(unsafe_code)]

//! # Dravr Channels
//!
//! A unified messaging interface for routing conversations through external chat
//! platforms (Whatsapp, Messenger, Discord, Slack, Telegram).
//!
//! ## Integration Modes
//!
//! 1. **Programmatic** — use the [`MessagingChannel`] trait directly in your Rust code
//! 2. **Server** — run `dravr-canot-server` as a standalone REST + MCP API server
//!
//! ## Architecture
//!
//! Each channel implements two traits:
//! - [`TransportAdapter`] — webhook parsing, signature verification, raw HTTP send
//! - [`ResponseRenderer`] — format [`OutgoingMessage`](models::OutgoingMessage) into channel-specific payloads

/// Messaging error types
pub mod error;

/// Data models: channel types, messages, delivery receipts, configuration
pub mod models;

/// Conversation-turn correlation identifier threaded through a user utterance
pub mod turn;

/// Runtime configuration loaded from environment variables
pub mod config;

/// Trait for pluggable channel configuration storage backends
pub mod config_store;

/// Environment-variable-based channel config store for standalone deployments
pub mod env_config_store;

/// Shared HTTP client for outbound API calls
pub mod http_client;

/// Channel trait combining transport and rendering
pub mod channel;

/// Channel descriptor metadata (name, type, webhook path, capabilities)
pub mod descriptor;

/// Transport adapter trait for webhook ingress and outbound HTTP
pub mod transport;

/// Response renderer trait for channel-specific message formatting
pub mod renderer;

/// Channel registry mapping `ChannelType` to adapter instances
pub mod registry;

/// Stateless adapter factory for on-demand channel adapter construction
pub mod factory;

/// Outbound retry worker with exponential backoff and dead-letter queue
pub mod retry;

/// Observability helpers for structured tracing spans and metrics
pub mod observability;

/// `MessageCounter` observer trait + `NoopMessageCounter` default.
///
/// Hosts inject a concrete `Arc<dyn MessageCounter>` to forward
/// inbound/outbound message events to their analytics backend
/// (PostHog, Datadog, etc.) without coupling the dispatch path to
/// any specific telemetry vendor.
pub mod counter;

/// Slash command infrastructure for markdown-defined messaging commands
pub mod commands;

/// Rich text formatting (HTML subset) translated per-channel into native markup
pub mod rich_text;

/// Shared Meta Platform (Messenger + Whatsapp) HMAC-SHA256 signature verification
pub mod meta_signature;

/// Per-channel adapter implementations
pub mod channels;

/// AG-UI SSE consumer for streaming pipeline progress to channel users.
///
/// Feature-gated behind `agui`; requires the `reqwest/stream`
/// back-pressure support. See module docs for the expected usage in
/// channel adapters that render per-stage status (e.g. Telegram).
#[cfg(feature = "agui")]
pub mod agui_consumer;

/// Channel-agnostic AG-UI status rendering.
///
/// Maps lifecycle / step / tool-call events into short human-readable
/// text and drives per-channel edit-message adapters (Telegram's
/// `editMessageText`, etc.).
#[cfg(feature = "agui")]
pub mod agui_status;

// Re-export primary types for consumers
pub use channel::MessagingChannel;
pub use config_store::ChannelConfigStore;
pub use counter::{MessageCounter, MessageDirection, NoopMessageCounter, SharedMessageCounter};
pub use descriptor::ChannelDescriptor;
pub use env_config_store::EnvConfigStore;
pub use registry::ChannelRegistry;
pub use renderer::ResponseRenderer;
pub use transport::TransportAdapter;
pub use turn::ConversationTurnId;
