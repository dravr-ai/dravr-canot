// ABOUTME: Observability helpers for structured tracing spans in messaging operations
// ABOUTME: Creates consistent spans for webhook processing, send attempts, and signature checks
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use tracing::{info_span, Span};

use crate::models::ChannelType;
use crate::turn::ConversationTurnId;

/// Create a tracing span for an inbound webhook processing pipeline
#[must_use]
pub fn webhook_received_span(channel: ChannelType, turn_id: ConversationTurnId) -> Span {
    info_span!(
        "messaging.webhook.received",
        channel = %channel,
        turn_id = %turn_id,
    )
}

/// Create a tracing span for an outbound send attempt
#[must_use]
pub fn send_attempt_span(channel: ChannelType, turn_id: ConversationTurnId, attempt: i32) -> Span {
    info_span!(
        "messaging.send.attempt",
        channel = %channel,
        turn_id = %turn_id,
        attempt = attempt,
    )
}

/// Create a tracing span for webhook signature verification
#[must_use]
pub fn signature_verification_span(channel: ChannelType) -> Span {
    info_span!(
        "messaging.signature.verify",
        channel = %channel,
    )
}
