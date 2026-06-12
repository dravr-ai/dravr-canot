// ABOUTME: Sender allowlist for gating inbound webhook messages by sender identity
// ABOUTME: Prevents prompt injection by only forwarding messages from approved senders
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashSet;

/// Sender allowlist that gates inbound messages by platform-specific sender ID.
///
/// The Claude Channels spec requires gating on the sender's identity
/// (not room/channel ID) to prevent prompt injection in group chats.
pub struct SenderGate {
    /// Set of allowed sender IDs (platform-specific: Slack user ID, Telegram user ID, etc.)
    allowed: HashSet<String>,
}

impl SenderGate {
    /// Create a gate from a comma-separated list of sender IDs.
    ///
    /// Empty or whitespace-only entries are silently skipped.
    pub fn from_csv(csv: &str) -> Self {
        let allowed = csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        Self { allowed }
    }

    /// Check whether a sender ID is in the allowlist.
    ///
    /// Returns `true` if the allowlist is empty (open mode) or if the sender is listed.
    pub fn is_allowed(&self, sender_id: &str) -> bool {
        self.allowed.is_empty() || self.allowed.contains(sender_id)
    }

    /// Number of sender IDs in the allowlist.
    pub fn len(&self) -> usize {
        self.allowed.len()
    }

    /// Whether the allowlist is empty (open mode — all senders permitted).
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
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
    use super::*;

    #[test]
    fn empty_csv_allows_all() {
        let gate = SenderGate::from_csv("");
        assert!(gate.is_empty());
        assert!(gate.is_allowed("anyone"));
    }

    #[test]
    fn csv_with_entries_gates_correctly() {
        let gate = SenderGate::from_csv("U123,U456");
        assert_eq!(gate.len(), 2);
        assert!(gate.is_allowed("U123"));
        assert!(gate.is_allowed("U456"));
        assert!(!gate.is_allowed("U789"));
    }

    #[test]
    fn trims_whitespace() {
        let gate = SenderGate::from_csv(" U123 , U456 ");
        assert!(gate.is_allowed("U123"));
        assert!(gate.is_allowed("U456"));
    }

    #[test]
    fn skips_empty_entries() {
        let gate = SenderGate::from_csv("U123,,U456,");
        assert_eq!(gate.len(), 2);
    }
}
