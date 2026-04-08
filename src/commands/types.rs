// ABOUTME: Core types for the messaging slash command system
// ABOUTME: Command definitions, parsed commands, execution context, and responses
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::fmt;

/// Required role to execute a command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRole {
    /// Any authenticated user
    Any,
    /// Must be a group member
    Member,
    /// Must be a group admin or owner
    Admin,
    /// Must be the group owner
    Owner,
}

impl CommandRole {
    /// Parse from string (used by YAML frontmatter)
    #[must_use]
    pub fn from_str_value(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "member" => Self::Member,
            "admin" => Self::Admin,
            "owner" => Self::Owner,
            _ => Self::Any,
        }
    }
}

impl fmt::Display for CommandRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any"),
            Self::Member => write!(f, "member"),
            Self::Admin => write!(f, "admin"),
            Self::Owner => write!(f, "owner"),
        }
    }
}

/// A command definition loaded from a markdown file
#[derive(Debug, Clone)]
pub struct CommandDefinition {
    /// Unique identifier (e.g., "group-status")
    pub name: String,
    /// The command string (e.g., "/group status")
    pub command: String,
    /// Alternative command strings (e.g., `["/gs"]`)
    pub aliases: Vec<String>,
    /// One-line description for help and Telegram bot menu
    pub description: String,
    /// Domain grouping (e.g., "general", "group", "coach", "data")
    pub domain: String,
    /// Required role to execute
    pub required_role: CommandRole,
    /// Whether the command requires an active group context
    pub requires_group: bool,
    /// Response template with `{variable}` placeholders
    pub response_template: String,
}

/// A parsed command from user input
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// The matched command definition
    pub name: String,
    /// Arguments after the command (split by whitespace)
    pub args: Vec<String>,
    /// The full raw message text
    pub raw_text: String,
}

/// Context passed to command handlers
#[derive(Debug, Clone)]
pub struct CommandContext {
    /// Authenticated user ID
    pub user_id: String,
    /// Active tenant ID
    pub tenant_id: String,
    /// Messaging channel type
    pub channel_type: String,
    /// Active group ID (if in a group context)
    pub group_id: Option<String>,
    /// Command arguments
    pub args: Vec<String>,
    /// Full raw message text
    pub raw_text: String,
}

/// An action button for card-style command responses
#[derive(Debug, Clone)]
pub struct CommandAction {
    /// Button label displayed to the user
    pub label: String,
    /// Action type: "postback" for callbacks, "url" for links
    pub action_type: String,
    /// Callback data or URL
    pub value: String,
}

/// Response from a command handler
#[derive(Debug, Clone)]
pub struct CommandResponse {
    /// Formatted response text to send back
    pub text: String,
    /// Whether this command requires a confirmation before executing
    pub require_confirmation: bool,
    /// Optional card title (when set, response renders as a card with buttons)
    pub card_title: Option<String>,
    /// Optional action buttons for card responses
    pub actions: Vec<CommandAction>,
}

impl CommandResponse {
    /// Create a simple text response
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            require_confirmation: false,
            card_title: None,
            actions: Vec::new(),
        }
    }

    /// Create a response that requires confirmation
    #[must_use]
    pub fn with_confirmation(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            require_confirmation: true,
            card_title: None,
            actions: Vec::new(),
        }
    }

    /// Create a card response with a title, body text, and action buttons
    #[must_use]
    pub fn card(
        title: impl Into<String>,
        body: impl Into<String>,
        actions: Vec<CommandAction>,
    ) -> Self {
        Self {
            text: body.into(),
            require_confirmation: false,
            card_title: Some(title.into()),
            actions,
        }
    }

    /// Whether this response should be rendered as a card with actions
    #[must_use]
    pub fn is_card(&self) -> bool {
        self.card_title.is_some() && !self.actions.is_empty()
    }
}

/// A pending confirmation for destructive commands
#[derive(Debug, Clone)]
pub struct PendingConfirmation {
    /// Command name that initiated the confirmation
    pub command_name: String,
    /// Original arguments
    pub args: Vec<String>,
    /// When the confirmation expires (ISO 8601)
    pub expires_at: String,
    /// Additional context needed to execute after confirmation
    pub context: std::collections::HashMap<String, String>,
}
