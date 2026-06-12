// ABOUTME: Conversation-turn correlation identifier threaded from webhook through downstream calls
// ABOUTME: Newtype over Uuid; wire format is a plain UUID string for cross-repo compatibility
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # Conversation Turn Identifier
//!
//! A conversation *turn* is one user utterance received from a channel
//! webhook plus the full chain of downstream LLM/tool calls that produces
//! the reply. Every step in that chain carries the same
//! [`ConversationTurnId`].
//!
//! The identifier is generated **once** at the webhook boundary and
//! propagated unchanged through the rest of the pipeline. Downstream
//! components must never generate a fresh identifier — doing so breaks
//! per-turn observability.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a single conversation turn.
///
/// Wire format: a standard UUID string. Wire-compatible with the
/// structurally-identical newtype in sibling crates (`embacle`,
/// `pierre-core`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationTurnId(pub Uuid);

impl ConversationTurnId {
    /// Generate a new random turn identifier.
    ///
    /// Only webhook ingress handlers (and other inbound boundaries) should
    /// call this. Downstream callers must propagate the identifier they
    /// received.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Wrap an existing UUID as a turn identifier.
    #[must_use]
    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    /// Return the underlying UUID.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ConversationTurnId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConversationTurnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for ConversationTurnId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ConversationTurnId> for Uuid {
    fn from(value: ConversationTurnId) -> Self {
        value.0
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::ConversationTurnId;

    #[test]
    fn serde_round_trip_is_plain_uuid_string() -> serde_json::Result<()> {
        let id = ConversationTurnId::new();
        let json = serde_json::to_string(&id)?;
        assert!(json.starts_with('"') && json.ends_with('"'));
        let parsed: ConversationTurnId = serde_json::from_str(&json)?;
        assert_eq!(id, parsed);
        Ok(())
    }

    #[test]
    fn new_ids_are_unique() {
        assert_ne!(ConversationTurnId::new(), ConversationTurnId::new());
    }
}
