// ABOUTME: MessageCounter observer trait for per-user/per-channel analytics aggregation
// ABOUTME: dravr-platform forwards observed events to PostHog when analytics_consent is on
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! # MessageCounter
//!
//! Observer surface a host application registers to receive per-user
//! / per-channel message-count events. The default implementation
//! ([`NoopMessageCounter`]) is a zero-cost no-op so feature gating
//! the analytics layer doesn't require touching the dispatch path.
//!
//! `dravr-platform` ships a concrete impl that buckets events into
//! PostHog's typed event bus. The trait stays in `dravr-canot` so
//! the dispatch + transport layers can call it without depending on
//! `dravr-platform`.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use dravr_canot::{MessageCounter, NoopMessageCounter, MessageDirection, ConversationTurnId};
//! use dravr_canot::models::ChannelType;
//!
//! let counter: Arc<dyn MessageCounter> = Arc::new(NoopMessageCounter);
//! counter.observe(MessageDirection::Inbound, ChannelType::Slack, "user-123", ConversationTurnId::new());
//! ```
//!
//! Hosts that want telemetry replace `NoopMessageCounter` with their
//! own `Arc<dyn MessageCounter>` at startup. The dispatch path treats
//! the trait object as opaque — failures inside the host's handler
//! should not propagate, so implementors must catch their own errors.

use std::sync::Arc;

use crate::models::ChannelType;
use crate::turn::ConversationTurnId;

/// Direction of the observed message — inbound or outbound.
///
/// Mirrors the natural vocabulary of
/// [`crate::models::IncomingMessage`] vs
/// [`crate::models::OutgoingMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    /// User → Platform.
    Inbound,
    /// Platform → User.
    Outbound,
}

impl MessageDirection {
    /// Stable string form for downstream telemetry buckets.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

/// Observer trait called once per successfully processed message.
///
/// The dispatch + transport paths invoke `observe` for each inbound
/// or outbound message. The trait is `Send + Sync` so a single
/// instance can be shared across the whole runtime via
/// [`Arc<dyn MessageCounter>`].
///
/// Implementors **must not** propagate errors out of `observe` —
/// telemetry failures should never break the message flow. Log + drop
/// any internal errors.
pub trait MessageCounter: Send + Sync {
    /// Record one message of the given direction on the given channel
    /// for the given user. The platform user id is passed as a string
    /// so transports that haven't resolved a UUID yet (early webhook
    /// parsing) can still call this with the channel-native id.
    fn observe(
        &self,
        direction: MessageDirection,
        channel: ChannelType,
        platform_user_id: &str,
        turn_id: ConversationTurnId,
    );
}

/// Default no-op observer. Used when the host has no telemetry
/// requirement, or when the user has not opted in to analytics. Zero
/// runtime cost — every method body is empty.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopMessageCounter;

impl MessageCounter for NoopMessageCounter {
    fn observe(
        &self,
        _direction: MessageDirection,
        _channel: ChannelType,
        _platform_user_id: &str,
        _turn_id: ConversationTurnId,
    ) {
        // Intentional no-op — see type-level docs.
    }
}

/// Shared `Arc<dyn MessageCounter>` ergonomic alias for the registry
/// + transport layers that hold a reference to the active observer.
pub type SharedMessageCounter = Arc<dyn MessageCounter>;

/// Build a default shared observer when none is configured. Returns
/// an `Arc<NoopMessageCounter>` upcast to the trait object.
#[must_use]
pub fn noop_shared() -> SharedMessageCounter {
    Arc::new(NoopMessageCounter)
}
