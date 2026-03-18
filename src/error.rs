// ABOUTME: Standalone messaging error types decoupled from pierre-core
// ABOUTME: Covers signature verification, delivery, payload parsing, and channel configuration errors
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
}

impl MessagingError {
    /// Whether this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::DeliveryFailed { retryable, .. } => *retryable,
            Self::ChannelApiError { status_code, .. } => {
                matches!(status_code, 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }
}

/// HTTP error response body for REST API error responses
#[derive(Debug, serde::Serialize)]
pub struct ErrorResponse {
    /// Error category
    pub error: ErrorBody,
}

/// Inner error body matching OpenAI error format
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
