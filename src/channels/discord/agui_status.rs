// ABOUTME: Discord implementation of StatusAdapter — renders AG-UI progress via PATCH /channels/.../messages/...
// ABOUTME: Sends an initial placeholder via POST messages, then edits it as pipeline stages advance
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Discord AG-UI status rendering.
//!
//! Flow per user turn:
//! 1. Channel adapter receives the user message.
//! 2. Adapter calls [`DiscordStatusAdapter::open`] which issues a
//!    `POST /channels/{channel_id}/messages` with the initial
//!    "thinking…" text; the REST API returns the new message's `id`.
//! 3. As the AG-UI consumer drains events, it forwards each one to
//!    [`crate::agui_status::drive_status_updates`] which calls
//!    [`DiscordStatusAdapter::set_status`]; each call issues a
//!    single `PATCH /channels/{channel_id}/messages/{message_id}` to
//!    refresh the placeholder.
//! 4. Once the platform's `RUN_FINISHED` event fires and the
//!    assistant reply text is known, the adapter calls
//!    [`DiscordStatusAdapter::finalize`] which edits the placeholder
//!    to the final reply (status + reply collapse into a single
//!    Discord message — no orphaned "thinking…" trail in channel
//!    history).

use crate::agui_status::StatusAdapter;
use crate::error::MessagingError;
use crate::http_client::api_client;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Default Discord REST API base URL. Production code never
/// overrides this; integration tests point it at a local mock
/// server.
const DEFAULT_REST_API_BASE: &str = "https://discord.com/api/v10";

/// Default minimum interval between `PATCH .../messages/...` calls
/// for the same channel.
///
/// Discord's per-channel rate limits sit around 5 messages per 5
/// seconds for writes; edits share a similar bucket and return
/// `X-RateLimit-Remaining` headers plus 429 `Retry-After` on
/// exhaustion. A chatty AG-UI stream (rapid `STEP_STARTED` /
/// `TOOL_CALL_START` sequence) would otherwise trigger 429s; the
/// edits that do land would be non-deterministic. Enforcing the
/// interval client-side means the user always sees the most recent
/// status transition, at the cost of dropping sub-second
/// intermediate ones they would not perceive anyway.
///
/// Overridable per-adapter via
/// [`DiscordStatusAdapter::with_edit_throttle`] so integration tests
/// that fire events faster than real Discord would can assert the
/// full transition set without tripping the client-side guard.
pub const DEFAULT_EDIT_MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// Discord status adapter for a single user turn.
///
/// Stateful: remembers the `message_id` of the initial placeholder
/// message so subsequent PATCH calls target the correct message.
pub struct DiscordStatusAdapter {
    http: Client,
    bot_token: String,
    channel_id: String,
    /// The `id` of the placeholder message, returned by
    /// `POST /channels/{channel_id}/messages`. PATCH uses it as the
    /// target message identifier.
    message_id: String,
    rest_api_base: String,
    /// Client-side minimum interval between throttled edits. Defaults
    /// to [`DEFAULT_EDIT_MIN_INTERVAL`]; tests swap in
    /// `Duration::ZERO` so rapid-fire event sequences can be
    /// asserted verbatim.
    edit_min_interval: Duration,
    /// Last edit timestamp — guards `edit_min_interval` to keep us
    /// under Discord's per-channel rate limit. `Mutex` rather than
    /// `AtomicInstant` because `Instant` isn't `Copy`-into-AtomicU64;
    /// contention is nil (one consumer task per adapter).
    last_edit_at: Mutex<Option<Instant>>,
}

impl DiscordStatusAdapter {
    /// Send the initial placeholder to `channel_id` and return an
    /// adapter bound to the resulting `message_id`.
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::DeliveryFailed`] when the REST API
    /// rejects the initial POST (invalid token, channel not
    /// reachable, rate-limited). The adapter is unusable in that
    /// state — callers must fall back to delivering the reply
    /// without progress feedback.
    pub async fn open(
        bot_token: impl Into<String>,
        channel_id: impl Into<String>,
        initial_text: &str,
    ) -> Result<Self, MessagingError> {
        Self::open_with_base(bot_token, channel_id, initial_text, DEFAULT_REST_API_BASE).await
    }

    /// `open`, but point at a custom Discord REST API base URL.
    ///
    /// Production code uses [`Self::open`]; this entry point exists
    /// so integration tests can aim at a local HTTP mock.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`].
    pub async fn open_with_base(
        bot_token: impl Into<String>,
        channel_id: impl Into<String>,
        initial_text: &str,
        rest_api_base: impl Into<String>,
    ) -> Result<Self, MessagingError> {
        let bot_token = bot_token.into();
        let channel_id = channel_id.into();
        let rest_api_base = rest_api_base.into();
        let http = api_client().clone();

        let body = json!({ "content": initial_text });
        let url = format!("{rest_api_base}/channels/{channel_id}/messages");
        let response = http
            .post(&url)
            .header("Authorization", format!("Bot {bot_token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "discord".into(),
                reason: format!("POST messages failed: {e}"),
                retryable: true,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MessagingError::DeliveryFailed {
                channel: "discord".into(),
                reason: format!("POST messages HTTP {status}: {text}"),
                retryable: status.is_server_error(),
            });
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "discord".into(),
                reason: format!("POST messages response parse failed: {e}"),
                retryable: false,
            })?;

        let message_id = payload
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| MessagingError::DeliveryFailed {
                channel: "discord".into(),
                reason: "POST messages response missing id".into(),
                retryable: false,
            })?
            .to_owned();

        Ok(Self {
            http,
            bot_token,
            channel_id,
            message_id,
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            rest_api_base,
        })
    }

    /// Construct from an already-existing placeholder message.
    ///
    /// Useful when the initial placeholder is posted by a caller
    /// that already has its own HTTP client and wants to hand the
    /// `message_id` over.
    #[must_use]
    pub fn from_message_id(
        bot_token: impl Into<String>,
        channel_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        Self::from_message_id_with_base(bot_token, channel_id, message_id, DEFAULT_REST_API_BASE)
    }

    /// `from_message_id`, but point at a custom REST API base URL.
    /// Production code should use [`Self::from_message_id`]; this
    /// entry point exists for integration tests.
    #[must_use]
    pub fn from_message_id_with_base(
        bot_token: impl Into<String>,
        channel_id: impl Into<String>,
        message_id: impl Into<String>,
        rest_api_base: impl Into<String>,
    ) -> Self {
        Self {
            http: api_client().clone(),
            bot_token: bot_token.into(),
            channel_id: channel_id.into(),
            message_id: message_id.into(),
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            rest_api_base: rest_api_base.into(),
        }
    }

    /// Override the client-side edit throttle interval.
    ///
    /// Production callers leave this at
    /// [`DEFAULT_EDIT_MIN_INTERVAL`]; integration tests that fire
    /// events faster than real Discord would set it to
    /// `Duration::ZERO` so every rendered event lands as an edit
    /// (makes assertions deterministic).
    #[must_use]
    pub const fn with_edit_throttle(mut self, interval: Duration) -> Self {
        self.edit_min_interval = interval;
        self
    }

    /// The Discord `message_id` of the placeholder message. Useful
    /// for tests and for callers that want to operate on the message
    /// independently of the adapter.
    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    /// Enforce the client-side edit throttle.
    ///
    /// Returns `true` when the caller should skip this edit because
    /// the last one happened within [`Self::edit_min_interval`];
    /// updates `last_edit_at` on a green signal so the next call's
    /// check is current. `Duration::ZERO` disables the throttle
    /// entirely.
    async fn should_throttle_edit(&self) -> bool {
        if self.edit_min_interval.is_zero() {
            return false;
        }
        let mut guard = self.last_edit_at.lock().await;
        let now = Instant::now();
        if let Some(last) = *guard {
            if now.duration_since(last) < self.edit_min_interval {
                return true;
            }
        }
        *guard = Some(now);
        false
    }

    /// Throttled edit — callers MUST drop the update if we've edited
    /// within [`DEFAULT_EDIT_MIN_INTERVAL`]. Used for intermediate
    /// status frames; the terminal [`Self::edit_message_force`]
    /// bypasses the throttle.
    async fn edit_message(&self, text: &str) -> Result<(), MessagingError> {
        if self.should_throttle_edit().await {
            debug!(
                channel_id = %self.channel_id,
                "discord PATCH messages throttled (client-side rate limit)"
            );
            return Ok(());
        }
        self.edit_message_force(text).await
    }

    /// Unthrottled edit — terminal path, used by `finalize` so the
    /// user's final reply lands regardless of recent status edits.
    async fn edit_message_force(&self, text: &str) -> Result<(), MessagingError> {
        // Update `last_edit_at` so subsequent throttled calls
        // account for the force-edit as well.
        *self.last_edit_at.lock().await = Some(Instant::now());

        let body = json!({ "content": text });
        let url = format!(
            "{}/channels/{}/messages/{}",
            self.rest_api_base, self.channel_id, self.message_id,
        );
        let response = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "discord".into(),
                reason: format!("PATCH messages failed: {e}"),
                retryable: true,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            warn!(
                channel_id = %self.channel_id,
                status = %status,
                body = %body_text,
                "discord PATCH messages non-2xx; skipping update"
            );
        }
        Ok(())
    }
}

#[async_trait]
impl StatusAdapter for DiscordStatusAdapter {
    async fn set_status(&self, text: &str) -> Result<(), MessagingError> {
        self.edit_message(text).await
    }

    async fn finalize(&self, reply: &str) -> Result<(), MessagingError> {
        // Bypass the client-side throttle so the terminal reply
        // always lands, even when `set_status` fired within the last
        // 1.1s.
        self.edit_message_force(reply).await
    }
}
