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

/// Duration in minutes before a link verification code expires
pub const LINK_CODE_TTL_MINUTES: i64 = 10;

/// OTP code expires after 10 minutes
pub const OTP_TTL_MINUTES: i64 = 10;

/// Maximum OTP verification attempts before flow is invalidated
pub const MAX_OTP_ATTEMPTS: i32 = 3;

/// Maximum OTP flows per hour per `channel_user_id` (rate limiting)
pub const MAX_OTP_FLOWS_PER_HOUR: i64 = 5;

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

impl ChannelType {
    /// Determine the linking method for this channel type
    #[must_use]
    pub const fn linking_method(self) -> LinkingMethod {
        match self {
            Self::Telegram | Self::WhatsApp => LinkingMethod::DeepLink,
            Self::Slack | Self::Discord | Self::Messenger => LinkingMethod::OAuth,
        }
    }
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
// Linking
// ============================================================================

/// Channel linking method: deep link (Telegram, Whatsapp) or oauth (Slack, Discord, Messenger)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkingMethod {
    /// Deep link with embedded verification code (Telegram `/start`, Whatsapp `LINK`)
    DeepLink,
    /// Standard oauth2 authorization code flow
    OAuth,
}

impl fmt::Display for LinkingMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeepLink => write!(f, "deep_link"),
            Self::OAuth => write!(f, "oauth"),
        }
    }
}

impl FromStr for LinkingMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "deep_link" => Ok(Self::DeepLink),
            "oauth" => Ok(Self::OAuth),
            other => Err(format!("unknown linking method: {other}")),
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
    /// Forum topic or thread identifier (e.g., Telegram `message_thread_id`)
    pub thread_id: Option<String>,
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
// Outbound Queue
// ============================================================================

/// Outbound queue entry for retry tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundQueueEntry {
    /// Queue entry identifier
    pub id: String,
    /// Reference to the original message
    pub message_id: String,
    /// Tenant identifier for isolation
    pub tenant_id: String,
    /// Target channel
    pub channel_type: ChannelType,
    /// Serialized outbound payload
    pub payload: Value,
    /// Current queue status (pending, retrying:N, sent, dlq)
    pub status: String,
    /// Number of delivery attempts made
    pub attempt_count: i32,
    /// Scheduled time for next retry attempt
    pub next_retry_at: Option<DateTime<Utc>>,
}

// ============================================================================
// Configuration
// ============================================================================

/// Per-tenant channel configuration for API access and webhook verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Configuration identifier
    pub id: String,
    /// Tenant identifier for isolation
    pub tenant_id: String,
    /// Channel platform type
    pub channel_type: ChannelType,
    /// API key or access token for outbound API calls
    pub api_key: Option<String>,
    /// API secret for signing outbound requests
    pub api_secret: Option<String>,
    /// Webhook secret for inbound signature verification
    pub webhook_secret: Option<String>,
    /// Meta webhook verify token (distinct from `webhook_secret` to avoid leaking HMAC key)
    pub verify_token: Option<String>,
    /// Platform account identifier (e.g., Discord application ID)
    pub account_id: Option<String>,
    /// Phone number identifier (Whatsapp/SMS)
    pub phone_number: Option<String>,
    /// Bot token for platforms that use separate bot credentials
    pub bot_token: Option<String>,
    /// Whether this channel configuration is active
    pub is_active: bool,
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

// ============================================================================
// Sessions
// ============================================================================

/// Active messaging session linking a channel user to a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingSession {
    /// Session identifier
    pub id: String,
    /// User identifier
    pub user_id: String,
    /// Tenant identifier
    pub tenant_id: String,
    /// Channel platform
    pub channel_type: ChannelType,
    /// Channel-specific user identifier
    pub channel_user_id: String,
    /// Channel-specific conversation or thread ID
    pub channel_conversation_id: Option<String>,
    /// Upstream conversation identifier
    pub conversation_id: Option<String>,
    /// Timestamp of last message activity
    pub last_message_at: DateTime<Utc>,
}

// ============================================================================
// Channel Linking
// ============================================================================

/// Ephemeral link state for a pending channel linking request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingLinkState {
    /// State identifier
    pub id: String,
    /// Tenant identifier
    pub tenant_id: String,
    /// User requesting the link
    pub user_id: String,
    /// Target channel platform
    pub channel_type: ChannelType,
    /// Cryptographically random verification code
    pub code: String,
    /// Linking method (`deep_link` or `oauth`)
    pub method: LinkingMethod,
    /// Whether this code has been consumed
    pub used: bool,
    /// Expiration timestamp (10 minutes from creation)
    pub expires_at: DateTime<Utc>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

/// Permanent mapping between a user and a messaging channel identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingChannelLink {
    /// Link identifier
    pub id: String,
    /// Tenant identifier
    pub tenant_id: String,
    /// User identifier
    pub user_id: String,
    /// Channel platform type
    pub channel_type: ChannelType,
    /// Channel-specific user identifier (phone number, platform user ID, etc.)
    pub channel_user_id: String,
    /// Human-readable display name from the platform
    pub display_name: Option<String>,
    /// Timestamp when the link was established
    pub linked_at: DateTime<Utc>,
}
