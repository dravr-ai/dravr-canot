// ABOUTME: Messaging slash command infrastructure for multi-platform channels
// ABOUTME: Types, registry, matcher, and formatter for markdown-defined commands
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # Slash Command System
//!
//! Provides infrastructure for defining and matching slash commands in messaging
//! conversations. Commands are defined as markdown files with YAML frontmatter
//! and loaded at startup into a [`CommandRegistry`].
//!
//! The system is channel-agnostic — the same commands work across Telegram,
//! WhatsApp, Discord, Slack, and Messenger.
//!
//! ## Architecture
//!
//! - **Types** — [`CommandDefinition`], [`ParsedCommand`], [`CommandResponse`]
//! - **Registry** — [`CommandRegistry`] maps command strings to definitions
//! - **Matcher** — [`CommandMatcher`] detects commands in message text
//! - **Formatter** — [`format_response`] applies template variable substitution

/// Core types for command definitions, parsed commands, and responses
pub mod types;

/// Command registry mapping command strings and aliases to definitions
pub mod registry;

/// Command matcher detecting slash commands in message text
pub mod matcher;

/// Response formatter with `{variable}` template substitution
pub mod formatter;

// Re-exports for convenience
pub use formatter::format_response;
pub use matcher::CommandMatcher;
pub use registry::CommandRegistry;
pub use types::{
    CommandContext, CommandDefinition, CommandResponse, CommandRole, ParsedCommand,
    PendingConfirmation,
};
