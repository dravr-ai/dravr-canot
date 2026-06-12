// ABOUTME: Integration tests for the AG-UI consumer — SSE frame parsing + event deserialization
// ABOUTME: Full HTTP stream tests live in the dravr-platform integration suite
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(feature = "agui")]
#![allow(missing_docs)]

use dravr_canot::agui_consumer::AgUiEvent;

/// AG-UI events deserialize from the server's JSON representation. The
/// consumer matches on variants directly so channel adapters can render
/// lifecycle, step, and tool-call events with no glue code.
#[test]
fn deserializes_run_started_event() {
    let payload = r#"{"type":"RUN_STARTED","run_id":"run_1","thread_id":"thread_1","timestamp":1}"#;
    let Ok(event) = serde_json::from_str::<AgUiEvent>(payload) else {
        panic!("must deserialize a RUN_STARTED payload");
    };
    match event {
        AgUiEvent::RunStarted {
            run_id, thread_id, ..
        } => {
            assert_eq!(run_id, "run_1");
            assert_eq!(thread_id.as_deref(), Some("thread_1"));
        }
        _ => panic!("expected RunStarted variant"),
    }
}

/// Step events surface the stage name so Telegram adapters can render
/// human-readable progress text ("calling `get_activities`…").
#[test]
fn deserializes_step_started_event() {
    let payload =
        r#"{"type":"STEP_STARTED","run_id":"run_1","step_name":"dispatch","timestamp":1}"#;
    let Ok(event) = serde_json::from_str::<AgUiEvent>(payload) else {
        panic!("must deserialize STEP_STARTED");
    };
    match event {
        AgUiEvent::StepStarted {
            run_id, step_name, ..
        } => {
            assert_eq!(run_id, "run_1");
            assert_eq!(step_name, "dispatch");
        }
        _ => panic!("expected StepStarted variant"),
    }
}

/// Unknown event kinds decode into the `Unknown` variant so the
/// consumer tolerates forward-compat events added to the AG-UI spec
/// after the library is released.
#[test]
fn unknown_event_kinds_route_to_unknown_variant() {
    let payload = r#"{"type":"FUTURE_SPEC_EVENT","run_id":"run_1","timestamp":1}"#;
    let Ok(event) = serde_json::from_str::<AgUiEvent>(payload) else {
        panic!("must deserialize unknown event kind");
    };
    assert!(matches!(event, AgUiEvent::Unknown));
}

/// `RUN_ERROR` surfaces both the machine code and the human message so
/// channel adapters can choose between compact and verbose rendering.
#[test]
fn deserializes_run_error_event() {
    let payload = r#"{"type":"RUN_ERROR","run_id":"run_1","code":"InternalError","message":"boom","timestamp":1}"#;
    let Ok(event) = serde_json::from_str::<AgUiEvent>(payload) else {
        panic!("must deserialize RUN_ERROR");
    };
    match event {
        AgUiEvent::RunError { code, message, .. } => {
            assert_eq!(code, "InternalError");
            assert_eq!(message, "boom");
        }
        _ => panic!("expected RunError variant"),
    }
}
