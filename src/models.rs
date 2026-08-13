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

use crate::turn::ConversationTurnId;

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
            // Messenger belongs here, not with the OAuth channels: its native
            // account-linking primitive is the m.me deep link, whose `ref`
            // comes back on the webhook exactly as Telegram's `?start=` does.
            // Classifying it as OAuth produced a linking URL that pointed at the
            // consumer's own callback with no way to supply the channel user id,
            // so every Messenger link attempt failed.
            Self::Telegram | Self::WhatsApp | Self::Messenger => LinkingMethod::DeepLink,
            Self::Slack | Self::Discord => LinkingMethod::OAuth,
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
    /// Rich text using a constrained HTML subset (`<b>`, `<i>`, `<code>`).
    ///
    /// Each channel renderer translates the markup into its native
    /// formatting (Telegram HTML, Slack mrkdwn, `WhatsApp` text formatting,
    /// Discord markdown, Messenger plaintext). Malformed input is
    /// preserved as literal text — see [`crate::rich_text`].
    RichText {
        /// Body text containing the HTML subset
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
    /// Human-readable chat / channel / group title (if available from the
    /// platform). Populated only for non-DM contexts where the platform
    /// exposes a name (Telegram `chat.title`, Slack channel name, Discord
    /// channel name, `WhatsApp` group subject). `None` for DMs or when the
    /// transport cannot resolve a title without an extra API call. Hosts
    /// use this to label the auto-bound `coaching_groups` row instead of
    /// falling back to the synthetic `{channel} group {chat_id}` form.
    /// Serde defaults to `None` so older payloads stay deserializable.
    #[serde(default)]
    pub chat_title: Option<String>,
    /// Platform-specific message identifier
    pub channel_message_id: String,
    /// Timestamp when the message was received
    pub timestamp: DateTime<Utc>,
    /// Raw webhook payload for debugging and audit
    pub raw_payload: Value,
    /// Conversation-turn correlation identifier.
    ///
    /// Generated once at the webhook boundary and threaded through every
    /// downstream call produced by this message. Never regenerated.
    pub turn_id: ConversationTurnId,
    /// `true` when the sender is talking to the bot in a one-to-one
    /// conversation (Telegram private chat, Slack IM, Discord DM channel,
    /// `WhatsApp`, Messenger, AGUI web chat). `false` for group / supergroup
    /// / channel / multi-party contexts. Populated per channel from the
    /// transport's native chat-kind signal; serde defaults to `false` on
    /// deserialization to keep older payloads compatible.
    #[serde(default)]
    pub is_direct_message: bool,
    /// `true` when this message explicitly addresses the bot: an @-mention
    /// of the bot's own identity, a reply to one of the bot's messages, or
    /// an interaction with a bot-owned UI element (inline-keyboard tap,
    /// Slack block action, Discord interaction). Direct messages are
    /// inherently addressed to the bot, so every DM carries `true`. Hosts
    /// use this to gate group replies to explicitly-addressed turns.
    /// Detection is best-effort per channel: Telegram needs the bot
    /// username (config `bot_username`) for @-mention matching and derives
    /// the bot id from `bot_token` for reply detection; Slack matches
    /// `<@U…>` mentions against the event's `authorizations`; Discord
    /// matches `mentions[]` / `referenced_message` against the gateway's
    /// READY user id. Serde defaults to `false` so older payloads stay
    /// deserializable.
    #[serde(default)]
    pub addressed_to_bot: bool,
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
    /// Conversation-turn correlation identifier carried from the inbound
    /// boundary. Populated by the caller; never generated here.
    pub turn_id: ConversationTurnId,
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
    /// Conversation-turn correlation identifier carried from the outbound
    /// message. Lets downstream observers (test harness, investigator)
    /// key off the receipt.
    pub turn_id: ConversationTurnId,
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
    /// Conversation-turn correlation identifier carried through the retry
    /// loop. Passed to `send_raw` on every attempt so the receipt stays
    /// keyed to the originating turn.
    pub turn_id: ConversationTurnId,
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
