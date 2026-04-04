// ABOUTME: Permission relay for forwarding Claude Code tool approval prompts to remote channels
// ABOUTME: Tracks pending requests by ID and parses yes/no verdict replies with replay protection
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;

use dravr_canot::models::{ChannelConfig, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::ChannelRegistry;
use regex::Regex;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

/// Parsed permission verdict from an inbound message
pub struct PermissionVerdict {
    /// The five-letter request ID echoed back from the user
    pub request_id: String,
    /// "allow" or "deny"
    pub behavior: String,
}

/// Routing context for a pending permission request, binding the request ID
/// to the specific channel/chat/sender that should receive the prompt and
/// whose verdict should be accepted.
struct PendingRequest {
    /// Channel type string (e.g., "slack")
    channel_type: String,
    /// Conversation/chat ID where the prompt was sent
    chat_id: String,
}

/// Parameters for a permission request to be relayed to a sender
pub struct PermissionRequestParams<'a> {
    /// The request ID from Claude Code
    pub request_id: &'a str,
    /// Tool name being requested
    pub tool_name: &'a str,
    /// Human-readable description of the tool action
    pub description: &'a str,
    /// Channel type of the sender to relay to (e.g., "slack")
    pub sender_channel_type: &'a str,
    /// Chat ID of the sender to relay to
    pub sender_chat_id: &'a str,
}

/// Permission relay that routes tool approval prompts to the sender who
/// triggered the conversation, tracks pending request IDs, and validates
/// verdict replies against outstanding requests to prevent spoofing/replay.
pub struct PermissionRelay {
    /// Outstanding permission requests keyed by request ID
    pending_requests: RwLock<HashMap<String, PendingRequest>>,
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
            pending_requests: RwLock::new(HashMap::new()),
            verdict_pattern,
        }
    }

    /// Try to parse an inbound message as a permission verdict and consume
    /// the pending request if the ID is valid.
    ///
    /// Returns `Some(PermissionVerdict)` only if the message matches the verdict
    /// format AND the request ID is actually pending. Consumed IDs cannot be
    /// replayed — a second verdict for the same ID returns `None`.
    pub async fn try_consume_verdict(
        &self,
        text: &str,
        channel_type: &str,
        chat_id: &str,
    ) -> Option<PermissionVerdict> {
        let caps = self.verdict_pattern.captures(text)?;
        let verdict_word = caps.get(1)?.as_str();
        let request_id = caps.get(2)?.as_str().to_lowercase();

        // Validate and consume the pending request atomically
        let mut pending = self.pending_requests.write().await;
        let entry = pending.remove(&request_id)?;

        // Verify the verdict comes from the same channel/chat where the prompt was sent
        if entry.channel_type != channel_type || entry.chat_id != chat_id {
            warn!(
                request_id = %request_id,
                prompt_channel = %entry.channel_type,
                verdict_channel = %channel_type,
                "Verdict from different channel/chat than prompt — rejecting"
            );
            // Re-insert so the correct sender can still respond
            pending.insert(request_id, entry);
            return None;
        }

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
    /// to the specified sender's channel and tracking the request ID.
    pub async fn handle_request(
        &self,
        params: &PermissionRequestParams<'_>,
        registry: &ChannelRegistry,
        configs: &HashMap<ChannelType, ChannelConfig>,
    ) {
        if params.request_id.is_empty() {
            warn!("Permission request with empty request_id — ignoring");
            return;
        }

        let channel_type: ChannelType = match params.sender_channel_type.parse() {
            Ok(ct) => ct,
            Err(e) => {
                error!(error = %e, "Invalid channel type for permission relay");
                return;
            }
        };

        // Register as pending before sending the prompt
        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(
                params.request_id.to_owned(),
                PendingRequest {
                    channel_type: params.sender_channel_type.to_owned(),
                    chat_id: params.sender_chat_id.to_owned(),
                },
            );
        }

        let prompt = format!(
            "Claude wants to run {}: {}\n\n\
             Reply \"yes {}\" or \"no {}\"",
            params.tool_name, params.description, params.request_id, params.request_id
        );

        let msg = OutgoingMessage {
            channel_type,
            recipient_id: params.sender_chat_id.to_owned(),
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
            Ok(_) => debug!(
                channel = %channel_type,
                request_id = %params.request_id,
                "Permission prompt relayed"
            ),
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

    // --- Verdict parsing tests (regex matching) ---

    #[tokio::test]
    async fn parses_yes_verdict_when_pending() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C123".to_owned(),
                },
            );
        }
        let v = r.try_consume_verdict("yes abcde", "slack", "C123").await;
        let v = v.expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "abcde");
        assert_eq!(v.behavior, "allow");
    }

    #[tokio::test]
    async fn parses_no_verdict_when_pending() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "xyzwv".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C123".to_owned(),
                },
            );
        }
        let v = r.try_consume_verdict("no xyzwv", "slack", "C123").await;
        let v = v.expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "xyzwv");
        assert_eq!(v.behavior, "deny");
    }

    #[tokio::test]
    async fn parses_short_yes() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        let v = r.try_consume_verdict("y abcde", "slack", "C1").await;
        assert_eq!(v.expect("should match").behavior, "allow"); // Safe: test assertion
    }

    #[tokio::test]
    async fn parses_short_no() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        let v = r.try_consume_verdict("n abcde", "slack", "C1").await;
        assert_eq!(v.expect("should match").behavior, "deny"); // Safe: test assertion
    }

    #[tokio::test]
    async fn case_insensitive() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        let v = r.try_consume_verdict("YES ABCDE", "slack", "C1").await;
        let v = v.expect("should match"); // Safe: test assertion
        assert_eq!(v.request_id, "abcde");
        assert_eq!(v.behavior, "allow");
    }

    #[tokio::test]
    async fn tolerates_whitespace() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        let v = r
            .try_consume_verdict("  yes   abcde  ", "slack", "C1")
            .await;
        assert_eq!(v.expect("should match").request_id, "abcde"); // Safe: test assertion
    }

    #[tokio::test]
    async fn rejects_no_id() {
        let r = relay();
        assert!(r.try_consume_verdict("yes", "slack", "C1").await.is_none());
    }

    #[tokio::test]
    async fn rejects_wrong_length_id() {
        let r = relay();
        assert!(r
            .try_consume_verdict("yes abcd", "slack", "C1")
            .await
            .is_none()); // 4 chars
        assert!(r
            .try_consume_verdict("yes abcdef", "slack", "C1")
            .await
            .is_none()); // 6 chars
    }

    #[tokio::test]
    async fn rejects_id_with_l() {
        let r = relay();
        // 'l' is excluded from the Claude Code ID alphabet
        assert!(r
            .try_consume_verdict("yes abcle", "slack", "C1")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn rejects_regular_messages() {
        let r = relay();
        assert!(r
            .try_consume_verdict("hello world", "slack", "C1")
            .await
            .is_none());
        assert!(r
            .try_consume_verdict("approve it", "slack", "C1")
            .await
            .is_none());
        assert!(r
            .try_consume_verdict("yes please do that", "slack", "C1")
            .await
            .is_none());
    }

    // --- Pending request tracking tests ---

    #[tokio::test]
    async fn reject_verdict_for_unknown_id() {
        let r = relay();
        // No pending request inserted — verdict should be rejected
        let result = r.try_consume_verdict("yes abcde", "slack", "C1").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn reject_replayed_verdict() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        // First verdict should succeed
        let first = r.try_consume_verdict("yes abcde", "slack", "C1").await;
        assert!(first.is_some());
        // Second verdict for same ID should be rejected (consumed)
        let second = r.try_consume_verdict("yes abcde", "slack", "C1").await;
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn reject_verdict_from_wrong_channel() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        // Verdict from telegram instead of slack — rejected
        let result = r.try_consume_verdict("yes abcde", "telegram", "C1").await;
        assert!(result.is_none());
        // Original pending request should still be there for the correct sender
        assert_eq!(r.pending_requests.read().await.len(), 1);
    }

    #[tokio::test]
    async fn reject_verdict_from_wrong_chat() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        // Verdict from different chat — rejected
        let result = r.try_consume_verdict("yes abcde", "slack", "C999").await;
        assert!(result.is_none());
        assert_eq!(r.pending_requests.read().await.len(), 1);
    }

    #[tokio::test]
    async fn consume_verdict_removes_pending() {
        let r = relay();
        {
            let mut pending = r.pending_requests.write().await;
            pending.insert(
                "abcde".to_owned(),
                PendingRequest {
                    channel_type: "slack".to_owned(),
                    chat_id: "C1".to_owned(),
                },
            );
        }
        assert_eq!(r.pending_requests.read().await.len(), 1);
        r.try_consume_verdict("yes abcde", "slack", "C1").await;
        assert_eq!(r.pending_requests.read().await.len(), 0);
    }
}
