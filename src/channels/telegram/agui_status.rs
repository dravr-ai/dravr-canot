// ABOUTME: Telegram implementation of StatusAdapter — renders AG-UI progress via editMessageText
// ABOUTME: Sends an initial placeholder message, then edits it in place as pipeline stages advance
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Telegram AG-UI status rendering.
//!
//! Flow per user turn:
//! 1. Channel adapter receives the user message.
//! 2. Adapter calls `TelegramStatusAdapter::open(chat_id)` which sends
//!    a `sendMessage` with the initial "thinking…" text; the Bot API
//!    returns a `message_id`.
//! 3. As the AG-UI consumer drains events, it forwards each one to
//!    [`crate::agui_status::drive_status_updates`] which calls
//!    [`TelegramStatusAdapter::set_status`]; each call issues a single
//!    `editMessageText` to refresh the placeholder.
//! 4. Once the platform's `RUN_FINISHED` event fires and the assistant
//!    reply text is known, the adapter calls
//!    [`TelegramStatusAdapter::finalize`] which edits the placeholder
//!    to the final reply (so the status and the reply collapse into a
//!    single Telegram message — no orphaned "thinking…" trail in chat
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

/// Default Telegram Bot API base URL. Production code never overrides
/// this; integration tests point it at a local mock server.
const DEFAULT_BOT_API_BASE: &str = "https://api.telegram.org";

/// Default minimum interval between `editMessageText` calls for the
/// same chat.
///
/// Telegram's Bot API caps edits at roughly 1 per second per chat before
/// returning 429 "Too Many Requests". A chatty AG-UI stream (rapid
/// `STEP_STARTED` / `TOOL_CALL_START` sequence) would otherwise trigger
/// rate-limiting on the server side; the edits that do land would be
/// non-deterministic because the 429s surface as non-2xx responses the
/// adapter logs-and-skips. Enforcing the interval client-side means the
/// user always sees the most recent status transition, at the cost of
/// dropping sub-second intermediate ones they would not perceive anyway.
///
/// Overridable per-adapter via [`TelegramStatusAdapter::with_edit_throttle`]
/// so integration tests that fire events faster than real Telegram
/// would can assert the full transition set without tripping the
/// client-side guard.
pub const DEFAULT_EDIT_MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// Telegram status adapter for a single user turn.
///
/// Stateful: remembers the `message_id` of the initial placeholder
/// message so subsequent edits target the correct message.
pub struct TelegramStatusAdapter {
    http: Client,
    bot_token: String,
    chat_id: String,
    message_thread_id: Option<i64>,
    message_id: i64,
    bot_api_base: String,
    /// Client-side minimum interval between throttled edits. Defaults
    /// to [`DEFAULT_EDIT_MIN_INTERVAL`]; tests swap in `Duration::ZERO`
    /// so rapid-fire event sequences can be asserted verbatim.
    edit_min_interval: Duration,
    /// Last edit timestamp — guards `edit_min_interval` to keep us
    /// under Telegram's ~1 req/s/chat rate limit. `Mutex` rather
    /// than `AtomicInstant` because `Instant` isn't `Copy`-into-AtomicU64;
    /// contention is nil (one consumer task per adapter).
    last_edit_at: Mutex<Option<Instant>>,
}

impl TelegramStatusAdapter {
    /// Send the initial "thinking…" placeholder to `chat_id` and
    /// return an adapter bound to the resulting `message_id`.
    ///
    /// `message_thread_id` is forwarded to `sendMessage` when the
    /// conversation happens inside a forum topic; pass `None` for
    /// direct chats and group chats without topics.
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::DeliveryFailed`] when the Bot API
    /// rejects the initial `sendMessage` call (invalid token, chat
    /// not reachable, rate-limited). The adapter is unusable in
    /// that state — callers must fall back to delivering the reply
    /// without progress feedback.
    pub async fn open(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        message_thread_id: Option<i64>,
        initial_text: &str,
    ) -> Result<Self, MessagingError> {
        Self::open_with_base(
            bot_token,
            chat_id,
            message_thread_id,
            initial_text,
            DEFAULT_BOT_API_BASE,
        )
        .await
    }

    /// `open`, but point at a custom Telegram Bot API base URL.
    ///
    /// Production code uses [`Self::open`]; this entry point exists
    /// so integration tests can aim at a local HTTP mock.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`].
    pub async fn open_with_base(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        message_thread_id: Option<i64>,
        initial_text: &str,
        bot_api_base: impl Into<String>,
    ) -> Result<Self, MessagingError> {
        let bot_token = bot_token.into();
        let chat_id = chat_id.into();
        let bot_api_base = bot_api_base.into();
        let http = api_client().clone();

        let mut body = json!({
            "chat_id": chat_id,
            "text": initial_text,
        });
        if let Some(thread_id) = message_thread_id {
            body["message_thread_id"] = json!(thread_id);
        }

        let url = format!("{bot_api_base}/bot{bot_token}/sendMessage");
        let response = http.post(&url).json(&body).send().await.map_err(|e| {
            MessagingError::DeliveryFailed {
                channel: "telegram".into(),
                reason: format!("sendMessage failed: {e}"),
                retryable: true,
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MessagingError::DeliveryFailed {
                channel: "telegram".into(),
                reason: format!("sendMessage HTTP {status}: {text}"),
                retryable: status.is_server_error(),
            });
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "telegram".into(),
                reason: format!("sendMessage response parse failed: {e}"),
                retryable: false,
            })?;

        let message_id = payload
            .pointer("/result/message_id")
            .and_then(Value::as_i64)
            .ok_or_else(|| MessagingError::DeliveryFailed {
                channel: "telegram".into(),
                reason: "sendMessage response missing result.message_id".into(),
                retryable: false,
            })?;

        Ok(Self {
            http,
            bot_token,
            chat_id,
            message_thread_id,
            message_id,
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            bot_api_base,
        })
    }

    /// Construct from an already-existing placeholder message.
    ///
    /// Useful when the initial "thinking…" is sent by a caller that
    /// already has its own HTTP client and wants to hand the
    /// `message_id` over.
    #[must_use]
    pub fn from_message_id(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        message_thread_id: Option<i64>,
        message_id: i64,
    ) -> Self {
        Self::from_message_id_with_base(
            bot_token,
            chat_id,
            message_thread_id,
            message_id,
            DEFAULT_BOT_API_BASE,
        )
    }

    /// `from_message_id`, but point at a custom Bot API base URL.
    /// Production code should use [`Self::from_message_id`]; this
    /// entry point exists for integration tests.
    #[must_use]
    pub fn from_message_id_with_base(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        message_thread_id: Option<i64>,
        message_id: i64,
        bot_api_base: impl Into<String>,
    ) -> Self {
        Self {
            http: api_client().clone(),
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
            message_thread_id,
            message_id,
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            bot_api_base: bot_api_base.into(),
        }
    }

    /// Override the client-side edit throttle interval.
    ///
    /// Production callers leave this at [`DEFAULT_EDIT_MIN_INTERVAL`];
    /// integration tests that fire events faster than real Telegram
    /// would set it to `Duration::ZERO` so every rendered event lands
    /// as an edit (makes assertions deterministic).
    #[must_use]
    pub const fn with_edit_throttle(mut self, interval: Duration) -> Self {
        self.edit_min_interval = interval;
        self
    }

    /// The Telegram `message_id` of the placeholder message. Useful
    /// for tests and for callers that want to operate on the message
    /// independently of the adapter.
    #[must_use]
    pub const fn message_id(&self) -> i64 {
        self.message_id
    }

    /// Enforce the client-side edit throttle.
    ///
    /// Returns `true` when the caller should skip this edit because
    /// the last one happened within [`Self::edit_min_interval`]; updates
    /// `last_edit_at` on a green signal so the next call's check is
    /// current. `Duration::ZERO` disables the throttle entirely.
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
    /// within [`EDIT_MIN_INTERVAL`]. Used for intermediate status
    /// frames; the terminal [`Self::edit_message_force`] bypasses the
    /// throttle.
    async fn edit_message(&self, text: &str) -> Result<(), MessagingError> {
        if self.should_throttle_edit().await {
            debug!(
                chat_id = %self.chat_id,
                "telegram editMessageText throttled (client-side rate limit)"
            );
            return Ok(());
        }
        self.edit_message_force(text).await
    }

    /// Unthrottled edit — terminal path, used by `finalize` so the
    /// user's final reply lands regardless of recent status edits.
    async fn edit_message_force(&self, text: &str) -> Result<(), MessagingError> {
        // Update `last_edit_at` so subsequent throttled calls account
        // for the force-edit as well.
        *self.last_edit_at.lock().await = Some(Instant::now());

        let mut body = json!({
            "chat_id": self.chat_id,
            "message_id": self.message_id,
            "text": text,
        });
        if let Some(thread_id) = self.message_thread_id {
            body["message_thread_id"] = json!(thread_id);
        }

        let url = format!(
            "{}/bot{}/editMessageText",
            self.bot_api_base, self.bot_token,
        );
        let response = self.http.post(&url).json(&body).send().await.map_err(|e| {
            MessagingError::DeliveryFailed {
                channel: "telegram".into(),
                reason: format!("editMessageText failed: {e}"),
                retryable: true,
            }
        })?;

        if !response.status().is_success() {
            // Telegram's API rejects edits with identical text with
            // HTTP 400 "message is not modified" — that is benign
            // (the status didn't actually change since the last
            // edit). Log at DEBUG and move on rather than surfacing
            // it to the consumer.
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if text.contains("message is not modified") {
                return Ok(());
            }
            warn!(
                chat_id = %self.chat_id,
                status = %status,
                body = %text,
                "telegram editMessageText non-2xx; skipping update"
            );
        }
        Ok(())
    }
}

#[async_trait]
impl StatusAdapter for TelegramStatusAdapter {
    async fn set_status(&self, text: &str) -> Result<(), MessagingError> {
        self.edit_message(text).await
    }

    async fn finalize(&self, reply: &str) -> Result<(), MessagingError> {
        // Bypass the client-side throttle so the terminal reply always
        // lands, even when `set_status` fired within the last 1.1s.
        self.edit_message_force(reply).await
    }
}
