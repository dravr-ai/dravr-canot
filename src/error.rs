// ABOUTME: Standalone messaging error types aligned with pierre-core
// ABOUTME: Covers signature verification, delivery, sessions, linking, and channel configuration errors
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::fmt;

/// Result type alias for messaging operations
pub type MessagingResult<T> = Result<T, MessagingError>;

/// Errors arising from messaging channel operations
#[derive(Debug, thiserror::Error)]
pub enum MessagingError {
    /// Webhook signature failed cryptographic verification
    #[error("[{channel}] signature verification failed: {reason}")]
    SignatureVerificationFailed {
        /// Channel that failed verification
        channel: String,
        /// Detailed failure reason
        reason: String,
    },

    /// Webhook timestamp is too old, indicating a potential replay attack
    #[error("[{channel}] replay detected: {reason}")]
    ReplayDetected {
        /// Channel that detected the replay
        channel: String,
        /// Detailed reason including timestamp age
        reason: String,
    },

    /// Duplicate `channel_message_id` already processed (idempotency guard)
    #[error("[{channel}] duplicate message {channel_message_id}")]
    DuplicateMessage {
        /// Channel type
        channel: String,
        /// The duplicate message identifier
        channel_message_id: String,
    },

    /// Message delivery to the channel API failed
    #[error("[{channel}] delivery failed: {reason}")]
    DeliveryFailed {
        /// Target channel
        channel: String,
        /// Failure reason
        reason: String,
        /// Whether the caller should retry
        retryable: bool,
    },

    /// All retry attempts exhausted, message moved to dead-letter queue
    #[error("[{channel}] delivery exhausted after {attempts} attempts: {reason}")]
    DeliveryExhausted {
        /// Target channel
        channel: String,
        /// Number of delivery attempts made
        attempts: i32,
        /// Last failure reason
        reason: String,
    },

    /// Channel API returned a rate limit response
    #[error("[{channel}] rate limit exceeded: retry after {retry_after_secs}s")]
    RateLimitExceeded {
        /// Channel that is rate-limited
        channel: String,
        /// Seconds to wait before retrying
        retry_after_secs: u64,
    },

    /// Channel is missing required configuration (API key, secret, etc.)
    #[error("[{channel}] channel not configured")]
    ChannelNotConfigured {
        /// Channel missing configuration
        channel: String,
    },

    /// Webhook payload could not be parsed
    #[error("[{channel}] invalid payload: {reason}")]
    InvalidPayload {
        /// Channel that received the invalid payload
        channel: String,
        /// Parse failure reason
        reason: String,
    },

    /// Messaging session not found
    #[error("session {session_id} not found")]
    SessionNotFound {
        /// Missing session identifier
        session_id: String,
    },

    /// Messaging session has expired
    #[error("session {session_id} expired")]
    SessionExpired {
        /// Expired session identifier
        session_id: String,
    },

    /// Media upload to channel failed
    #[error("[{channel}] media upload failed: {reason}")]
    MediaUploadFailed {
        /// Target channel
        channel: String,
        /// Upload failure reason
        reason: String,
    },

    /// Channel API returned a non-success HTTP status
    #[error("[{channel}] API error (HTTP {status_code}): {message}")]
    ChannelApiError {
        /// Channel whose API returned the error
        channel: String,
        /// HTTP status code from the channel API
        status_code: u16,
        /// Error message from the channel API response body
        message: String,
    },

    /// Link verification code has expired (past TTL)
    #[error("link code has expired")]
    LinkCodeExpired,

    /// Link verification code has already been consumed
    #[error("link code has already been used")]
    LinkCodeAlreadyUsed,

    /// Channel identity is already linked to a user in this tenant
    #[error("[{channel}] identity {channel_user_id} is already linked")]
    ChannelAlreadyLinked {
        /// Channel type that is already linked
        channel: String,
        /// Channel-specific user identifier
        channel_user_id: String,
    },

    /// Link code cannot be completed (already has a `user_id` set)
    #[error("link code {code} cannot be completed: {reason}")]
    LinkCodeNotCompletable {
        /// The link code that cannot be completed
        code: String,
        /// Reason the completion failed
        reason: String,
    },

    /// Attempted to unlink a channel that is not linked
    #[error("[{channel}] channel is not linked for this user")]
    ChannelNotLinked {
        /// Channel type that is not linked
        channel: String,
    },
}

impl MessagingError {
    /// Whether this error is retryable
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::DeliveryFailed { retryable, .. } => *retryable,
            Self::RateLimitExceeded { .. } => true,
            Self::ChannelApiError { status_code, .. } => {
                matches!(status_code, 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }

    /// Seconds to wait before retrying, if applicable
    pub const fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimitExceeded {
                retry_after_secs, ..
            } => Some(*retry_after_secs),
            _ => None,
        }
    }
}

/// HTTP error response body for REST API error responses
#[derive(Debug, serde::Serialize)]
pub struct ErrorResponse {
    /// Error category
    pub error: ErrorBody,
}

/// Inner error body matching the openai error format
#[derive(Debug, serde::Serialize)]
pub struct ErrorBody {
    /// Error type identifier
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message
    pub message: String,
}

impl ErrorResponse {
    /// Create an error response with the given type and message
    pub fn new(error_type: impl Into<String>, message: impl fmt::Display) -> Self {
        Self {
            error: ErrorBody {
                error_type: error_type.into(),
                message: message.to_string(),
            },
        }
    }
}
