// ABOUTME: Command matcher that detects slash commands in message text
// ABOUTME: Greedy longest-match algorithm with argument extraction
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use super::registry::CommandRegistry;
use super::types::ParsedCommand;

/// Matches incoming message text against registered commands.
///
/// Uses greedy longest-match: `/group status` is matched before `/group`.
/// Non-command messages (not starting with `/`) return `None` immediately.
pub struct CommandMatcher {
    /// Command strings sorted by length descending for greedy matching
    sorted_commands: Vec<String>,
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

        let lower = trimmed.to_lowercase();

        // Greedy match: try longest commands first
        for cmd in &self.sorted_commands {
            if lower.starts_with(cmd.as_str()) {
                // Ensure the match is at a word boundary
                let rest = &trimmed[cmd.len()..];
                if rest.is_empty() || rest.starts_with(' ') {
                    let def = registry.get_by_command(cmd)?;
                    let args: Vec<String> =
                        rest.split_whitespace().map(ToOwned::to_owned).collect();

                    return Some(ParsedCommand {
                        name: def.name.clone(),
                        args,
                        raw_text: trimmed.to_owned(),
                    });
                }
            }
        }

        None
    }
}
