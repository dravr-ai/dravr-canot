// ABOUTME: Slack implementation of StatusAdapter — renders AG-UI progress via chat.update
// ABOUTME: Sends an initial placeholder via chat.postMessage, then edits it as pipeline stages advance
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! Slack AG-UI status rendering.
//!
//! Flow per user turn:
//! 1. Channel adapter receives the user message.
//! 2. Adapter calls [`SlackStatusAdapter::open`] which sends a
//!    `chat.postMessage` with the initial "thinking…" text; the
//!    Web API returns a `ts` (Slack's message identifier).
//! 3. As the AG-UI consumer drains events, it forwards each one to
//!    [`crate::agui_status::drive_status_updates`] which calls
//!    [`SlackStatusAdapter::set_status`]; each call issues a single
//!    `chat.update` to refresh the placeholder.
//! 4. Once the platform's `RUN_FINISHED` event fires and the assistant
//!    reply text is known, the adapter calls
//!    [`SlackStatusAdapter::finalize`] which edits the placeholder to
//!    the final reply (status + reply collapse into a single Slack
//!    message — no orphaned "thinking…" trail in channel history).

use crate::agui_status::StatusAdapter;
use crate::error::MessagingError;
use crate::http_client::api_client;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};

/// Default Slack Web API base URL. Production code never overrides
/// this; integration tests point it at a local mock server.
const DEFAULT_WEB_API_BASE: &str = "https://slack.com/api";

/// Default minimum interval between `chat.update` calls for the same
/// channel.
///
/// Slack's own guidance is "no more than one message per second per
/// channel" for apps; `chat.update` sits in the same per-channel
/// budget as `chat.postMessage`. A chatty AG-UI stream (rapid
/// `STEP_STARTED` / `TOOL_CALL_START` sequence) would otherwise
/// trigger 429 `ratelimited` responses; the edits that do land would
/// be non-deterministic because retry-after windows surface as
/// non-`ok` bodies the adapter logs-and-skips. Enforcing the
/// interval client-side means the user always sees the most recent
/// status transition, at the cost of dropping sub-second
/// intermediate ones they would not perceive anyway.
///
/// Overridable per-adapter via [`SlackStatusAdapter::with_edit_throttle`]
/// so integration tests that fire events faster than real Slack
/// would can assert the full transition set without tripping the
/// client-side guard.
pub const DEFAULT_EDIT_MIN_INTERVAL: Duration = Duration::from_millis(1100);

/// Slack status adapter for a single user turn.
///
/// Stateful: remembers the `ts` (Slack message timestamp, which is
/// also the message identifier) of the initial placeholder message
/// so subsequent `chat.update` calls target the correct message.
pub struct SlackStatusAdapter {
    http: Client,
    bot_token: String,
    channel: String,
    /// The `ts` of the placeholder message, returned by
    /// `chat.postMessage`. `chat.update` uses it as the target
    /// message identifier — Slack does not expose a separate numeric
    /// `message_id` the way Telegram does.
    ///
    /// Threading is NOT tracked on the adapter: `chat.update` targets
    /// the message by `(channel, ts)` alone and Slack preserves the
    /// original thread placement. The `thread_ts` accepted by
    /// [`Self::open`] is forwarded once to `chat.postMessage` so the
    /// placeholder lands in the right thread; no further bookkeeping
    /// is needed.
    ts: String,
    web_api_base: String,
    /// Client-side minimum interval between throttled edits. Defaults
    /// to [`DEFAULT_EDIT_MIN_INTERVAL`]; tests swap in `Duration::ZERO`
    /// so rapid-fire event sequences can be asserted verbatim.
    edit_min_interval: Duration,
    /// Last edit timestamp — guards `edit_min_interval` to keep us
    /// under Slack's ~1 req/s/channel practical limit. `Mutex` rather
    /// than `AtomicInstant` because `Instant` isn't `Copy`-into-AtomicU64;
    /// contention is nil (one consumer task per adapter).
    last_edit_at: Mutex<Option<Instant>>,
}

impl SlackStatusAdapter {
    /// Send the initial placeholder to `channel` and return an
    /// adapter bound to the resulting `ts`.
    ///
    /// `thread_ts` is forwarded to `chat.postMessage` when the
    /// conversation happens inside a thread; pass `None` for
    /// top-level channel messages.
    ///
    /// # Errors
    ///
    /// Returns [`MessagingError::DeliveryFailed`] when the Web API
    /// rejects the initial `chat.postMessage` call (invalid token,
    /// channel not reachable, rate-limited). The adapter is unusable
    /// in that state — callers must fall back to delivering the reply
    /// without progress feedback.
    pub async fn open(
        bot_token: impl Into<String>,
        channel: impl Into<String>,
        thread_ts: Option<String>,
        initial_text: &str,
    ) -> Result<Self, MessagingError> {
        Self::open_with_base(
            bot_token,
            channel,
            thread_ts,
            initial_text,
            DEFAULT_WEB_API_BASE,
        )
        .await
    }

    /// `open`, but point at a custom Slack Web API base URL.
    ///
    /// Production code uses [`Self::open`]; this entry point exists
    /// so integration tests can aim at a local HTTP mock.
    ///
    /// # Errors
    ///
    /// Same as [`Self::open`].
    pub async fn open_with_base(
        bot_token: impl Into<String>,
        channel: impl Into<String>,
        thread_ts: Option<String>,
        initial_text: &str,
        web_api_base: impl Into<String>,
    ) -> Result<Self, MessagingError> {
        let bot_token = bot_token.into();
        let channel = channel.into();
        let web_api_base = web_api_base.into();
        let http = api_client().clone();

        let mut body = json!({
            "channel": channel,
            "text": initial_text,
        });
        if let Some(ref thread) = thread_ts {
            body["thread_ts"] = json!(thread);
        }

        let url = format!("{web_api_base}/chat.postMessage");
        let response = http
            .post(&url)
            .header("Authorization", format!("Bearer {bot_token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: format!("chat.postMessage failed: {e}"),
                retryable: true,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: format!("chat.postMessage HTTP {status}: {text}"),
                retryable: status.is_server_error(),
            });
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: format!("chat.postMessage response parse failed: {e}"),
                retryable: false,
            })?;

        // Slack returns 200 OK even for logical failures; the real
        // verdict is the `ok` field. Treat `ok: false` as a delivery
        // failure so callers know the placeholder never landed.
        if !payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let error = payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: format!("chat.postMessage ok=false: {error}"),
                retryable: false,
            });
        }

        let ts = payload
            .get("ts")
            .and_then(Value::as_str)
            .ok_or_else(|| MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: "chat.postMessage response missing ts".into(),
                retryable: false,
            })?
            .to_owned();

        Ok(Self {
            http,
            bot_token,
            channel,
            ts,
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            web_api_base,
        })
    }

    /// Construct from an already-existing placeholder message.
    ///
    /// Useful when the initial placeholder is posted by a caller that
    /// already has its own HTTP client and wants to hand the `ts`
    /// over. Thread placement is already baked into the placeholder;
    /// `chat.update` targets the message by `(channel, ts)` and
    /// preserves thread context on its own.
    #[must_use]
    pub fn from_ts(
        bot_token: impl Into<String>,
        channel: impl Into<String>,
        ts: impl Into<String>,
    ) -> Self {
        Self::from_ts_with_base(bot_token, channel, ts, DEFAULT_WEB_API_BASE)
    }

    /// `from_ts`, but point at a custom Web API base URL. Production
    /// code should use [`Self::from_ts`]; this entry point exists for
    /// integration tests.
    #[must_use]
    pub fn from_ts_with_base(
        bot_token: impl Into<String>,
        channel: impl Into<String>,
        ts: impl Into<String>,
        web_api_base: impl Into<String>,
    ) -> Self {
        Self {
            http: api_client().clone(),
            bot_token: bot_token.into(),
            channel: channel.into(),
            ts: ts.into(),
            edit_min_interval: DEFAULT_EDIT_MIN_INTERVAL,
            last_edit_at: Mutex::new(None),
            web_api_base: web_api_base.into(),
        }
    }

    /// Override the client-side edit throttle interval.
    ///
    /// Production callers leave this at [`DEFAULT_EDIT_MIN_INTERVAL`];
    /// integration tests that fire events faster than real Slack
    /// would set it to `Duration::ZERO` so every rendered event
    /// lands as an edit (makes assertions deterministic).
    #[must_use]
    pub const fn with_edit_throttle(mut self, interval: Duration) -> Self {
        self.edit_min_interval = interval;
        self
    }

    /// The Slack `ts` of the placeholder message. Useful for tests
    /// and for callers that want to operate on the message
    /// independently of the adapter.
    #[must_use]
    pub fn ts(&self) -> &str {
        &self.ts
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
                channel = %self.channel,
                "slack chat.update throttled (client-side rate limit)"
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

        let body = json!({
            "channel": self.channel,
            "ts": self.ts,
            "text": text,
        });

        let url = format!("{}/chat.update", self.web_api_base);
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.bot_token))
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "slack".into(),
                reason: format!("chat.update failed: {e}"),
                retryable: true,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            warn!(
                channel = %self.channel,
                status = %status,
                body = %body_text,
                "slack chat.update non-2xx; skipping update"
            );
            return Ok(());
        }

        // Slack returns 200 OK with `{"ok": false, "error": "..."}` on
        // logical failures. `message_not_found` happens when the user
        // deletes the placeholder; `cant_update_message` when the
        // workspace revoked chat:write. Neither should tear down the
        // consumer — log and move on.
        let payload: Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    channel = %self.channel,
                    error = %e,
                    "slack chat.update response parse failed; skipping update"
                );
                return Ok(());
            }
        };
        if !payload.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let error = payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            warn!(
                channel = %self.channel,
                error = %error,
                "slack chat.update ok=false; skipping update"
            );
        }
        Ok(())
    }
}

#[async_trait]
impl StatusAdapter for SlackStatusAdapter {
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
