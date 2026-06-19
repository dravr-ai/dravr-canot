// ABOUTME: AG-UI protocol consumer that subscribes to an SSE run stream and reports progress
// ABOUTME: Channel adapters call AgUiConsumer::stream() with a callback to render status updates
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! AG-UI protocol consumer.
//!
//! Subscribes to an [AG-UI](https://github.com/ag-ui-protocol/ag-ui)
//! Server-Sent Events run stream served by `dravr-platform`
//! (`GET /api/agui/runs/{run_id}/stream`) and invokes a caller-supplied
//! callback for every decoded event.
//!
//! Channel adapters use this to render per-stage progress updates in
//! the user-facing surface. The canonical Telegram integration replaces
//! "assistant is typing…" with the name of the current pipeline stage
//! (e.g. "calling `get_activities`", "generating response").
//!
//! # Example
//!
//! ```no_run
//! use dravr_canot::agui_consumer::{AgUiConsumer, AgUiEvent};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let consumer = AgUiConsumer::new(
//!     "https://pierre.example.com".parse()?,
//!     "bearer_token_here",
//! );
//!
//! consumer
//!     .stream("run_abc", |event| async move {
//!         match event {
//!             AgUiEvent::StepStarted { step_name, .. } => {
//!                 println!("\u{2192} {step_name}");
//!             }
//!             AgUiEvent::RunFinished { .. } => {
//!                 println!("done");
//!             }
//!             _ => {}
//!         }
//!     })
//!     .await?;
//! # Ok(()) }
//! ```

use crate::error::MessagingError;
use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::str;
use tracing::{debug, warn};

/// Decoded AG-UI event received from the server.
///
/// Mirrors the wire-format schema defined by
/// <https://github.com/ag-ui-protocol/ag-ui>. Consumers pattern-match on
/// variants they care about; unknown variants route to [`Self::Unknown`]
/// so forward-compat new event kinds do not break clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgUiEvent {
    /// A new agent run has started.
    #[serde(rename = "RUN_STARTED")]
    RunStarted {
        /// Run identifier.
        run_id: String,
        /// Conversation/thread the run belongs to.
        #[serde(default)]
        thread_id: Option<String>,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// A logical step inside a run has started.
    #[serde(rename = "STEP_STARTED")]
    StepStarted {
        /// Run identifier.
        run_id: String,
        /// Human-readable step name.
        step_name: String,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// A logical step inside a run has finished.
    #[serde(rename = "STEP_FINISHED")]
    StepFinished {
        /// Run identifier.
        run_id: String,
        /// Step name matching a prior `STEP_STARTED`.
        step_name: String,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// The run completed successfully.
    #[serde(rename = "RUN_FINISHED")]
    RunFinished {
        /// Run identifier.
        run_id: String,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// The run terminated with an error.
    #[serde(rename = "RUN_ERROR")]
    RunError {
        /// Run identifier.
        run_id: String,
        /// Short machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// The agent invoked a tool.
    #[serde(rename = "TOOL_CALL_START")]
    ToolCallStart {
        /// Run identifier.
        run_id: String,
        /// Unique identifier for this tool call.
        tool_call_id: String,
        /// Name of the tool being invoked.
        tool_name: String,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// Tool execution produced a result.
    #[serde(rename = "TOOL_CALL_RESULT")]
    ToolCallResult {
        /// Run identifier.
        run_id: String,
        /// Tool call identifier matching a prior `TOOL_CALL_START`.
        tool_call_id: String,
        /// Result payload.
        result: Value,
        /// Milliseconds since the Unix epoch.
        timestamp: u64,
    },
    /// Any event kind the consumer does not understand. The raw JSON
    /// object is preserved so callers can inspect the `type` field and
    /// handle new AG-UI events without a library upgrade.
    #[serde(other)]
    Unknown,
}

/// AG-UI run stream subscriber.
///
/// Configured with the Pierre platform base URL and a bearer token.
/// [`Self::stream`] opens a long-running HTTP SSE connection for a
/// given `run_id` and invokes the caller-supplied callback per event.
#[derive(Clone)]
pub struct AgUiConsumer {
    http: Client,
    base_url: Url,
    bearer_token: String,
}

impl AgUiConsumer {
    /// Construct a consumer bound to `base_url` using `bearer_token`
    /// for Authorization on every subscription request.
    ///
    /// # Panics
    ///
    /// Panics only if the underlying `reqwest` client cannot be built,
    /// which requires an environment misconfiguration (missing TLS
    /// backend) rather than a runtime error condition.
    #[must_use]
    pub fn new(base_url: Url, bearer_token: impl Into<String>) -> Self {
        // SSE streams are long-lived. Reqwest's default HTTP client
        // applies no overall request timeout when `.timeout(...)` is
        // not set, which is what AG-UI subscribers want — the remote
        // peer's 15-second keep-alive frame enforces liveness.
        let http = Client::builder()
            .user_agent(concat!("dravr-canot/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            base_url,
            bearer_token: bearer_token.into(),
        }
    }

    /// Subscribe to `run_id` and call `on_event` for every decoded
    /// event until the server closes the stream or `on_event` returns.
    ///
    /// The callback receives an owned [`AgUiEvent`] so it can hand the
    /// event to another async task (e.g. a Telegram update job).
    ///
    /// # Errors
    ///
    /// Returns a [`MessagingError::DeliveryFailed`] when the initial
    /// handshake fails or the stream yields an unparseable chunk.
    /// Network transport errors mid-stream end the subscription
    /// without returning an error — the server owns the run lifecycle
    /// and closure is the normal termination signal.
    pub async fn stream<F, Fut>(&self, run_id: &str, on_event: F) -> Result<(), MessagingError>
    where
        F: Fn(AgUiEvent) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        let url = self
            .base_url
            .join(&format!("api/agui/runs/{run_id}/stream"))
            .map_err(|e| MessagingError::InvalidPayload {
                channel: "agui".into(),
                reason: format!("invalid AG-UI stream URL: {e}"),
            })?;

        let response = self
            .http
            .get(url)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.bearer_token),
            )
            .header(header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|e| MessagingError::DeliveryFailed {
                channel: "agui".into(),
                reason: format!("AG-UI stream connect failed: {e}"),
                retryable: true,
            })?;

        if response.status() != StatusCode::OK {
            return Err(MessagingError::DeliveryFailed {
                channel: "agui".into(),
                reason: format!("AG-UI stream returned HTTP {}", response.status()),
                retryable: response.status().is_server_error(),
            });
        }

        let mut parser = SseFrameParser::default();
        let mut body = response.bytes_stream();

        while let Some(chunk) = body.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    warn!(run_id = %run_id, error = %e, "AG-UI stream chunk error");
                    break;
                }
            };
            for frame in parser.feed(&bytes) {
                // Only `agui` data frames carry JSON AG-UI events.
                // Lifecycle frames (`connection`, `keepalive`) are
                // informational and intentionally skipped.
                if frame.event_name() != "agui" {
                    continue;
                }
                match serde_json::from_str::<AgUiEvent>(&frame.data) {
                    Ok(event) => on_event(event).await,
                    Err(e) => {
                        warn!(
                            run_id = %run_id,
                            error = %e,
                            data = %frame.data,
                            "skipping undecodable AG-UI frame"
                        );
                    }
                }
            }
        }

        debug!(run_id = %run_id, "AG-UI stream closed");
        Ok(())
    }
}

/// Minimal framed-SSE parser (`text/event-stream`).
///
/// Accumulates chunk bytes, splits by the spec-mandated double LF, and
/// returns complete frames as they appear. `event:` and `data:` lines
/// are supported; other field names are silently ignored.
#[derive(Debug, Default)]
struct SseFrameParser {
    buffer: String,
}

impl SseFrameParser {
    fn feed(&mut self, bytes: &[u8]) -> Vec<SseFrame> {
        if let Ok(chunk) = str::from_utf8(bytes) {
            self.buffer.push_str(chunk);
        }

        let mut frames = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let raw_frame = self.buffer[..end].to_owned();
            self.buffer.drain(..=end + 1);

            let mut event_name = String::new();
            let mut data_lines: Vec<&str> = Vec::new();
            for line in raw_frame.split('\n') {
                if let Some(rest) = line.strip_prefix("event:") {
                    rest.trim().clone_into(&mut event_name);
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim());
                }
            }

            if data_lines.is_empty() {
                continue;
            }
            frames.push(SseFrame {
                event: event_name,
                data: data_lines.join("\n"),
            });
        }
        frames
    }
}

#[derive(Debug)]
struct SseFrame {
    /// Value of the `event:` SSE field (empty when the server did not
    /// emit one). Retained so callers inspecting the raw frame can
    /// distinguish lifecycle frames (`connection`, `keepalive`) from
    /// payload frames (`agui`) when debugging the stream.
    event: String,
    data: String,
}

impl SseFrame {
    /// Name of the SSE `event:` field, or `""` when absent.
    #[must_use]
    fn event_name(&self) -> &str {
        &self.event
    }
}
