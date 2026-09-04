// ABOUTME: AG-UI protocol event schema decoded on the consumer side of a pipeline run
// ABOUTME: Channel status adapters match on these variants to render per-stage progress text
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! AG-UI protocol events, consumer side.
//!
//! [`dravr-platform`](https://github.com/dravr-ai/dravr-platform) produces
//! [AG-UI](https://github.com/ag-ui-protocol/ag-ui) events as a messaging
//! turn advances and hands them to this crate in-process, over the run
//! registry's broadcast. This module owns the wire schema those events
//! deserialize into.
//!
//! [`crate::agui_status`] maps a decoded event to a short status line and
//! drives the channel's edit-in-place primitive (Telegram
//! `editMessageText`, Slack `chat.update`, Discord `PATCH messages`) so
//! the user sees the current pipeline stage — "calling `get_activities`",
//! "generating response" — in the message they are already looking at.
//!
//! # Example
//!
//! ```
//! use dravr_canot::agui_consumer::AgUiEvent;
//!
//! let wire = r#"{"type":"STEP_STARTED","run_id":"run_abc","step_name":"dispatch","timestamp":5}"#;
//! let event: AgUiEvent = serde_json::from_str(wire)?;
//!
//! assert!(matches!(
//!     event,
//!     AgUiEvent::StepStarted { ref step_name, .. } if step_name == "dispatch"
//! ));
//! # Ok::<(), serde_json::Error>(())
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Any event kind the consumer does not understand. Deserialization
    /// succeeds and the payload is discarded, so an AG-UI event kind
    /// added after this library was released widens the stream rather
    /// than failing it; [`crate::agui_status::status_text_for_event`]
    /// renders nothing for it.
    #[serde(other)]
    Unknown,
}
