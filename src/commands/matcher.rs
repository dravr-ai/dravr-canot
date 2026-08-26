// ABOUTME: Command matcher that detects slash commands in message text
// ABOUTME: Greedy longest-match with alias canonicalisation so subcommands survive an alias
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::Arc;

use super::registry::CommandRegistry;
use super::types::{CommandDefinition, ParsedCommand};

/// Matches incoming message text against registered commands.
///
/// Uses greedy longest-match: `/group status` is matched before `/group`.
/// Non-command messages (not starting with `/`) return `None` immediately.
///
/// An alias stands for its canonical command, subcommands included. A
/// subcommand is registered under the canonical spelling only (`/coach invite`),
/// so text that reaches a definition through one of its aliases is resolved
/// again as if the canonical command had been typed: `/coaches invite` becomes
/// `/coach invite`, which then wins as the longer match. Without that second
/// pass the shorter alias would match, and `invite` would be handed to the
/// wrong handler as an argument.
pub struct CommandMatcher {
    /// Command strings sorted by length descending for greedy matching
    sorted_commands: Vec<String>,
}

/// One greedy prefix match: the definition, the lookup key that matched it,
/// and the text that followed the key.
struct PrefixMatch {
    definition: Arc<CommandDefinition>,
    key: String,
    rest: String,
}

impl CommandMatcher {
    /// Create a matcher from a command registry
    #[must_use]
    pub fn from_registry(registry: &CommandRegistry) -> Self {
        Self {
            sorted_commands: registry.sorted_commands(),
        }
    }

    /// Try to match a message against registered commands.
    ///
    /// Returns `None` if the message doesn't start with `/` or
    /// no registered command matches.
    #[must_use]
    pub fn try_match(&self, text: &str, registry: &CommandRegistry) -> Option<ParsedCommand> {
        let trimmed = text.trim();

        // Fast path: not a command
        if !trimmed.starts_with('/') {
            return None;
        }

        let first = self.longest_prefix(trimmed, registry)?;
        let matched = self.canonicalise(first, registry);

        let args: Vec<String> = matched
            .rest
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();

        Some(ParsedCommand {
            name: matched.definition.name.clone(),
            args,
            raw_text: trimmed.to_owned(),
        })
    }

    /// Re-resolve a match reached through an alias against the canonical
    /// spelling, so a subcommand typed after the alias is found.
    ///
    /// A match on the canonical command itself, or an alias with nothing after
    /// it, is already final. Canonical commands are never aliases, so a single
    /// pass suffices.
    fn canonicalise(&self, found: PrefixMatch, registry: &CommandRegistry) -> PrefixMatch {
        let canonical = found.definition.command.to_lowercase();
        if canonical == found.key || found.rest.is_empty() {
            return found;
        }
        let rewritten = format!("{canonical}{}", found.rest);
        self.longest_prefix(&rewritten, registry).unwrap_or(found)
    }

    /// Greedy longest-prefix match over every command string and alias,
    /// case-insensitive on the key and word-boundary aware.
    ///
    /// `rest` keeps the caller's original casing so arguments such as names
    /// reach the handler as typed.
    fn longest_prefix(&self, text: &str, registry: &CommandRegistry) -> Option<PrefixMatch> {
        let lower = text.to_lowercase();
        self.sorted_commands.iter().find_map(|cmd| {
            if !lower.starts_with(cmd.as_str()) {
                return None;
            }
            // Ensure the match is at a word boundary
            let rest = text.get(cmd.len()..)?;
            if !(rest.is_empty() || rest.starts_with(' ')) {
                return None;
            }
            let definition = Arc::clone(registry.get_by_command(cmd)?);
            Some(PrefixMatch {
                definition,
                key: cmd.clone(),
                rest: rest.to_owned(),
            })
        })
    }
}
