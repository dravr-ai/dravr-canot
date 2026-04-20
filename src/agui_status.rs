// ABOUTME: Channel-agnostic rendering of AG-UI events into human-readable status text
// ABOUTME: Adapters push the text into their channel's equivalent of "edit message" (e.g. Telegram editMessageText)
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! AG-UI progress rendering for messaging channels.
//!
//! Channel adapters (Telegram, Slack, Discord, ...) pick up
//! [`AgUiEvent`](crate::agui_consumer::AgUiEvent)s from
//! [`crate::agui_consumer::AgUiConsumer`] and render a single
//! short "the assistant is doing X" status line per event.
//!
//! The mapping from event → text lives in [`status_text_for_event`]
//! so all channels share the same vocabulary; what differs per
//! channel is how the text is delivered: Telegram edits the original
//! status message via `editMessageText`; Slack updates a placeholder
//! Block Kit card; Discord edits a followup.
//!
//! The [`StatusAdapter`] trait is the channel-specific contract.
//! Channel crates implement it once and forward consumer events to
//! [`StatusAdapter::set_status`]; the consumer handles fan-in and
//! rate-limits (channels typically have their own edit-rate caps).

use crate::agui_consumer::AgUiEvent;
use crate::error::MessagingError;
use async_trait::async_trait;

/// Render an AG-UI event into user-facing status text, or `None`
/// when the event should not surface visibly (e.g. content deltas
/// that are cheaper to ignore than to edit).
///
/// Keep the text short (<= 60 chars) so the message fits on a
/// single line in every messaging channel. Callers are free to
/// prefix / suffix branding (emoji, ellipsis).
#[must_use]
pub fn status_text_for_event(event: &AgUiEvent) -> Option<String> {
    match event {
        AgUiEvent::RunStarted { .. } => Some("thinking…".to_owned()),
        AgUiEvent::StepStarted { step_name, .. } => Some(match step_name.as_str() {
            "prompt_assembly" => "reading your question…".to_owned(),
            "dispatch" => "generating response…".to_owned(),
            other => format!("{other}…"),
        }),
        AgUiEvent::ToolCallStart { tool_name, .. } => Some(format!("calling {tool_name}…")),
        // Tool-call result: clear the "calling foo…" sticky status so the
        // user sees the tool advanced rather than a stuck placeholder. We
        // use a short "thinking…" to bridge back to whatever the next
        // step emits; a failed tool will follow up with either a fresh
        // `ToolCallStart` (retry) or a `RunError` that overwrites this.
        AgUiEvent::ToolCallResult { .. } => Some("thinking…".to_owned()),
        AgUiEvent::RunError { message, .. } => Some(format!("error: {message}")),
        AgUiEvent::StepFinished { .. } | AgUiEvent::RunFinished { .. } | AgUiEvent::Unknown => None,
    }
}

/// Channel-specific status delivery adapter.
///
/// A minimal interface so every messaging channel can plug in a
/// single render of the pipeline's current stage. Implementations
/// are stateful (they remember the `channel_message_id` of the
/// original "thinking…" message) and are created once per turn.
#[async_trait]
pub trait StatusAdapter: Send + Sync {
    /// Replace the current status text.
    ///
    /// # Errors
    ///
    /// Returns a [`MessagingError`] when the channel API rejects the
    /// update. Callers MUST swallow transient errors and keep
    /// consuming events — progress rendering is best-effort.
    async fn set_status(&self, text: &str) -> Result<(), MessagingError>;

    /// Finalize the status with the assistant's complete reply.
    ///
    /// Implementations decide whether this replaces the status
    /// message in place or sends a fresh message; Telegram typically
    /// does an `editMessageText` to the final text; other channels
    /// may delete the status and post the reply as a new message.
    ///
    /// # Errors
    ///
    /// Returns a [`MessagingError`] when the channel API rejects the
    /// final update.
    async fn finalize(&self, reply: &str) -> Result<(), MessagingError>;
}

/// Render a single [`AgUiEvent`] into a status update on `adapter`.
///
/// Maps the event through [`status_text_for_event`] and forwards the
/// result to [`StatusAdapter::set_status`]. Events that do not map to
/// user-visible text (e.g. `STEP_FINISHED`, `RUN_FINISHED`, `Unknown`)
/// are skipped.
///
/// # Lifecycle responsibility
///
/// This function does **not** call [`StatusAdapter::finalize`]. Finalize
/// is a terminal transition that collapses the status into the final
/// assistant reply — which only the caller has (it comes from the
/// out-of-band chat response, not from the AG-UI stream). The consumer
/// owns the loop and calls `finalize` once after the stream closes:
///
/// ```ignore
/// while let Some(event) = consumer.next().await {
///     drive_status_updates(&adapter, &event).await?;
/// }
/// adapter.finalize(&assistant_reply).await?;
/// ```
///
/// # Errors
///
/// Returns an error only on unrecoverable channel failures — transient
/// edit-rate failures should be logged by the adapter and silently
/// skipped.
pub async fn drive_status_updates(
    adapter: &dyn StatusAdapter,
    event: &AgUiEvent,
) -> Result<(), MessagingError> {
    if let Some(text) = status_text_for_event(event) {
        adapter.set_status(&text).await?;
    }
    Ok(())
}
