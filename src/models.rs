// ABOUTME: Standalone messaging data models decoupled from pierre-core
// ABOUTME: Channel types, message content, delivery receipts, and configuration structs
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ============================================================================
// Constants
// ============================================================================

/// Retry delay schedule in seconds (exponential backoff: 1s, 5s, 30s)
pub const RETRY_DELAYS_SECS: [u64; 3] = [1, 5, 30];

/// Maximum number of delivery retry attempts before dead-lettering
pub const MAX_RETRY_ATTEMPTS: i32 = 3;

// ============================================================================
// Channel Type
// ============================================================================

/// Supported messaging channel platforms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    /// Whatsapp Business Cloud API
    WhatsApp,
    /// Meta Messenger Platform
    Messenger,
    /// Discord Bot API
    Discord,
    /// Slack Events API
    Slack,
    /// Telegram Bot API
    Telegram,
}

impl fmt::Display for ChannelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhatsApp => write!(f, "whatsapp"),
            Self::Messenger => write!(f, "messenger"),
            Self::Discord => write!(f, "discord"),
            Self::Slack => write!(f, "slack"),
            Self::Telegram => write!(f, "telegram"),
        }
    }
}

impl FromStr for ChannelType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "whatsapp" => Ok(Self::WhatsApp),
            "messenger" => Ok(Self::Messenger),
            "discord" => Ok(Self::Discord),
            "slack" => Ok(Self::Slack),
            "telegram" => Ok(Self::Telegram),
            other => Err(format!("unknown channel type: {other}")),
        }
    }
}

// ============================================================================
// Message Content
// ============================================================================

/// Content variants for inbound and outbound messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    /// Plain text message
    Text {
        /// Message body text
        body: String,
    },
    /// Media attachment (image, video, audio, document)
    Media {
        /// Media URL or file identifier
        url: String,
        /// MIME type (e.g., "image/jpeg")
        mime_type: String,
        /// Optional caption
        caption: Option<String>,
    },
    /// Geographic location
    Location {
        /// Latitude coordinate
        latitude: f64,
        /// Longitude coordinate
        longitude: f64,
    },
    /// Rich card with title, body, and action buttons
    Card {
        /// Card title
        title: String,
        /// Card body text
        body: String,
        /// Interactive action buttons
        actions: Vec<CardAction>,
    },
}

/// An interactive button action within a Card message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardAction {
    /// Button label displayed to the user
    pub label: String,
    /// Action type: "url" for links, "postback" for callback actions
    pub action_type: String,
    /// Action value: URL or callback data
    pub value: String,
}

// ============================================================================
// Messages
// ============================================================================

/// A normalized inbound message received from a webhook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// Source channel platform
    pub channel_type: ChannelType,
    /// Platform-specific sender identifier
    pub sender_id: String,
    /// Human-readable sender name (if available from the platform)
    pub sender_name: Option<String>,
    /// Parsed message content
    pub content: MessageContent,
    /// Platform-specific conversation/thread identifier
    pub conversation_id: Option<String>,
    /// Platform-specific message identifier
    pub channel_message_id: String,
    /// Timestamp when the message was received
    pub timestamp: DateTime<Utc>,
    /// Raw webhook payload for debugging and audit
    pub raw_payload: Value,
    /// Correlation ID for distributed tracing
    pub correlation_id: Uuid,
    /// Additional platform-specific metadata
    pub metadata: Value,
}

/// An outbound message to be sent through a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMessage {
    /// Target channel platform
    pub channel_type: ChannelType,
    /// Platform-specific recipient identifier
    pub recipient_id: String,
    /// Message content to send
    pub content: MessageContent,
    /// Correlation ID for distributed tracing
    pub correlation_id: Uuid,
    /// Message ID to reply to (platform-specific threading)
    pub reply_to: Option<String>,
}

// ============================================================================
// Delivery
// ============================================================================

/// Status of an outbound message delivery attempt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryStatus {
    /// Queued for delivery
    Pending,
    /// Accepted by the channel API
    Sent,
    /// Confirmed delivered to the recipient
    Delivered,
    /// Read by the recipient
    Read,
    /// Delivery failed
    Failed,
    /// Moved to dead-letter queue after exhausting retries
    Dlq,
}

/// Receipt returned after a message delivery attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    /// Internal message identifier
    pub message_id: String,
    /// Platform-specific message identifier (if available)
    pub channel_message_id: Option<String>,
    /// Delivery outcome status
    pub status: DeliveryStatus,
    /// Timestamp of the delivery attempt
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Configuration
// ============================================================================

/// Channel-specific configuration for API access and webhook verification
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Configuration row identifier
    #[serde(default)]
    pub id: Option<String>,
    /// Tenant identifier for multi-tenant deployments
    #[serde(default)]
    pub tenant_id: Option<String>,
    /// Channel platform type
    #[serde(default)]
    pub channel_type: Option<ChannelType>,
    /// API key or access token for outbound API calls
    #[serde(default)]
    pub api_key: Option<String>,
    /// API secret for signing outbound requests
    #[serde(default)]
    pub api_secret: Option<String>,
    /// Webhook secret for inbound signature verification
    #[serde(default)]
    pub webhook_secret: Option<String>,
    /// Webhook verification token (used by some platforms during setup)
    #[serde(default)]
    pub verify_token: Option<String>,
    /// Platform account identifier (e.g., Discord application ID)
    #[serde(default)]
    pub account_id: Option<String>,
    /// Phone number identifier (Whatsapp)
    #[serde(default)]
    pub phone_number: Option<String>,
    /// Bot token for platforms that use separate bot credentials
    #[serde(default)]
    pub bot_token: Option<String>,
    /// Whether this channel configuration is active
    #[serde(default = "default_true")]
    pub is_active: bool,
}

const fn default_true() -> bool {
    true
}

/// Policy for webhook timestamp validation to prevent replay attacks
#[derive(Debug, Clone)]
pub struct WebhookTimestampPolicy {
    /// Maximum allowed age of a webhook timestamp in seconds
    pub max_age_secs: u64,
}

impl Default for WebhookTimestampPolicy {
    fn default() -> Self {
        Self { max_age_secs: 300 }
    }
}
