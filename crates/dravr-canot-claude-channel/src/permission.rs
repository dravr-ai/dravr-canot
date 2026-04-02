// ABOUTME: Permission relay for forwarding Claude Code tool approval prompts to remote channels
// ABOUTME: Tracks the last active sender and parses yes/no verdict replies
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;

use dravr_canot::models::{ChannelConfig, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::ChannelRegistry;
use regex::Regex;
use tokio::sync::RwLock;
use tracing::{debug, error};

/// Parsed permission verdict from an inbound message
pub struct PermissionVerdict {
    /// The five-letter request ID echoed back from the user
    pub request_id: String,
    /// "allow" or "deny"
    pub behavior: String,
}

/// Tracks the last active sender for routing permission prompts
struct LastActiveSender {
    /// Channel type string (e.g., "slack")
    channel_type: String,
    /// Conversation/chat ID
    chat_id: String,
}

/// Permission relay that routes tool approval prompts to the last active sender
/// and parses verdict replies from inbound messages.
pub struct PermissionRelay {
    /// The last sender who sent a message, tracked for permission routing
    last_active: RwLock<Option<LastActiveSender>>,
    /// Compiled regex for matching verdict replies (e.g., "yes abcde" or "no xyzwv")
    verdict_pattern: Regex,
}

impl PermissionRelay {
    /// Create a permission relay instance
    pub fn build() -> Self {
        // Matches "y abcde", "yes abcde", "n abcde", "no abcde"
        // [a-km-z] is the ID alphabet Claude Code uses (lowercase, skips 'l')
        // Case-insensitive to tolerate phone autocorrect
        let verdict_pattern =
            Regex::new(r"(?i)^\s*(y|yes|n|no)\s+([a-km-z]{5})\s*$").expect("valid regex"); // Safe: hardcoded valid regex pattern
        Self {
            last_active: RwLock::new(None),
            verdict_pattern,
        }
    }

    /// Record the last active sender for permission relay routing
    pub async fn set_last_active_sender(
        &self,
        channel_type: &str,
        chat_id: &str,
        _sender_id: &str,
    ) {
        let mut guard = self.last_active.write().await;
        *guard = Some(LastActiveSender {
            channel_type: channel_type.to_owned(),
            chat_id: chat_id.to_owned(),
        });
    }

    /// Try to parse an inbound message as a permission verdict.
    ///
    /// Returns `Some(PermissionVerdict)` if the message matches the verdict
    /// format ("yes <id>" or "no <id>"), `None` otherwise.
    pub fn try_parse_verdict(&self, text: &str) -> Option<PermissionVerdict> {
        let caps = self.verdict_pattern.captures(text)?;
        let verdict_word = caps.get(1)?.as_str();
        let request_id = caps.get(2)?.as_str().to_lowercase();

        let behavior = if verdict_word.to_lowercase().starts_with('y') {
            "allow"
        } else {
            "deny"
        };

        Some(PermissionVerdict {
            request_id,
            behavior: behavior.to_owned(),
        })
    }

    /// Handle a permission request from Claude Code by forwarding the prompt
    /// to the last active sender's channel.
    pub async fn handle_request(
        &self,
        request_id: &str,
        tool_name: &str,
        description: &str,
        registry: &ChannelRegistry,
        configs: &HashMap<ChannelType, ChannelConfig>,
    ) {
        let guard = self.last_active.read().await;
        let Some(last) = guard.as_ref() else {
            debug!("Permission request received but no active sender to relay to");
            return;
        };

        let channel_type: ChannelType = match last.channel_type.parse() {
            Ok(ct) => ct,
            Err(e) => {
                error!(error = %e, "Invalid channel type for permission relay");
                return;
            }
        };

        let prompt = format!(
            "Claude wants to run {tool_name}: {description}\n\n\
             Reply \"yes {request_id}\" or \"no {request_id}\""
        );

        let msg = OutgoingMessage {
            channel_type,
            recipient_id: last.chat_id.clone(),
            content: MessageContent::Text { body: prompt },
            correlation_id: uuid::Uuid::new_v4(),
            reply_to: None,
        };

        let Some(adapter) = registry.get(&channel_type) else {
            error!(channel = %channel_type, "No adapter registered for permission relay");
            return;
        };

        let Some(config) = configs.get(&channel_type) else {
            error!(channel = %channel_type, "No config found for permission relay");
            return;
        };

        match adapter.send(&msg, config).await {
            Ok(_) => debug!(channel = %channel_type, "Permission prompt relayed"),
            Err(e) => error!(error = %e, "Failed to relay permission prompt"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay() -> PermissionRelay {
        PermissionRelay::build()
    }

    #[test]
    fn parses_yes_verdict() {
        let r = relay();
        let v = r.try_parse_verdict("yes abcde").expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "abcde");
        assert_eq!(v.behavior, "allow");
    }

    #[test]
    fn parses_no_verdict() {
        let r = relay();
        let v = r.try_parse_verdict("no xyzwv").expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "xyzwv");
        assert_eq!(v.behavior, "deny");
    }

    #[test]
    fn parses_short_yes() {
        let r = relay();
        let v = r.try_parse_verdict("y abcde").expect("should match"); // Safe: test assertion
        assert_eq!(v.behavior, "allow");
    }

    #[test]
    fn parses_short_no() {
        let r = relay();
        let v = r.try_parse_verdict("n abcde").expect("should match"); // Safe: test assertion
        assert_eq!(v.behavior, "deny");
    }

    #[test]
    fn case_insensitive() {
        let r = relay();
        let v = r.try_parse_verdict("YES ABCDE").expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "abcde");
        assert_eq!(v.behavior, "allow");
    }

    #[test]
    fn tolerates_whitespace() {
        let r = relay();
        let v = r
            .try_parse_verdict("  yes   abcde  ")
            .expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "abcde");
    }

    #[test]
    fn rejects_no_id() {
        let r = relay();
        assert!(r.try_parse_verdict("yes").is_none());
    }

    #[test]
    fn rejects_wrong_length_id() {
        let r = relay();
        assert!(r.try_parse_verdict("yes abcd").is_none()); // 4 chars
        assert!(r.try_parse_verdict("yes abcdef").is_none()); // 6 chars
    }

    #[test]
    fn rejects_id_with_l() {
        let r = relay();
        // 'l' is excluded from the Claude Code ID alphabet
        assert!(r.try_parse_verdict("yes abcle").is_none());
    }

    #[test]
    fn rejects_regular_messages() {
        let r = relay();
        assert!(r.try_parse_verdict("hello world").is_none());
        assert!(r.try_parse_verdict("approve it").is_none());
        assert!(r.try_parse_verdict("yes please do that").is_none());
    }

    #[tokio::test]
    async fn tracks_last_active_sender() {
        let r = relay();
        r.set_last_active_sender("slack", "C123", "U456").await;
        let guard = r.last_active.read().await;
        let last = guard.as_ref().expect("should have last active"); // Safe: test assertion
        assert_eq!(last.channel_type, "slack");
        assert_eq!(last.chat_id, "C123");
    }
}
