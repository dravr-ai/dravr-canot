// ABOUTME: Command registry mapping command strings and aliases to definitions
// ABOUTME: Thread-safe lookup with case-insensitive matching and Telegram bot command export
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::types::CommandDefinition;

/// Registry of all available slash commands.
///
/// Built at startup from parsed markdown files. Provides O(1) lookup
/// by command string or alias, case-insensitive.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    /// Canonical command name → definition
    definitions: HashMap<String, Arc<CommandDefinition>>,
    /// Command string (including aliases) → canonical name
    lookup: HashMap<String, String>,
}

impl CommandRegistry {
    /// Create an empty registry
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command definition.
    ///
    /// Returns false if the command name is already registered.
    pub fn register(&mut self, def: CommandDefinition) -> bool {
        let name = def.name.clone();
        if self.definitions.contains_key(&name) {
            return false;
        }

        // Index the canonical command
        let cmd_lower = def.command.to_lowercase();
        self.lookup.insert(cmd_lower, name.clone());

        // Index all aliases
        for alias in &def.aliases {
            self.lookup.insert(alias.to_lowercase(), name.clone());
        }

        self.definitions.insert(name, Arc::new(def));
        true
    }

    /// Look up a command definition by canonical name
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<&Arc<CommandDefinition>> {
        self.definitions.get(name)
    }

    /// Look up a command definition by command string or alias
    #[must_use]
    pub fn get_by_command(&self, command: &str) -> Option<&Arc<CommandDefinition>> {
        let key = command.to_lowercase();
        self.lookup
            .get(&key)
            .and_then(|name| self.definitions.get(name))
    }

    /// Get all registered command definitions
    #[must_use]
    pub fn all_commands(&self) -> Vec<&Arc<CommandDefinition>> {
        self.definitions.values().collect()
    }

    /// Get commands filtered by domain
    #[must_use]
    pub fn commands_by_domain(&self, domain: &str) -> Vec<&Arc<CommandDefinition>> {
        self.definitions
            .values()
            .filter(|d| d.domain == domain)
            .collect()
    }

    /// Get all unique domain names
    #[must_use]
    pub fn domains(&self) -> Vec<String> {
        let mut domains: Vec<String> = self
            .definitions
            .values()
            .map(|d| d.domain.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        domains.sort();
        domains
    }

    /// Generate the command list for Telegram `setMyCommands` API.
    ///
    /// Returns `Vec<(command_without_slash, description)>`.
    /// Only includes top-level commands (single word after `/`).
    #[must_use]
    pub fn bot_command_list(&self) -> Vec<(String, String)> {
        let mut commands: Vec<(String, String)> = self
            .definitions
            .values()
            .filter(|d| !d.command.contains(' ') || d.command.matches(' ').count() == 0)
            .map(|d| {
                let cmd = d.command.trim_start_matches('/').to_owned();
                (cmd, d.description.clone())
            })
            .collect();
        commands.sort_by(|a, b| a.0.cmp(&b.0));
        commands.dedup_by(|a, b| a.0 == b.0);
        commands
    }

    /// Get all lookup keys sorted by length descending (for greedy matching)
    #[must_use]
    pub fn sorted_commands(&self) -> Vec<String> {
        let mut cmds: Vec<String> = self.lookup.keys().cloned().collect();
        cmds.sort_by_key(|b| Reverse(b.len()));
        cmds
    }

    /// Number of registered commands
    #[must_use]
    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    /// Whether the registry is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}
