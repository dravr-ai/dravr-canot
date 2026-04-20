// ABOUTME: End-to-end test of AG-UI SSE consumption driving Slack chat.update status edits
// ABOUTME: Stands up mock SSE source and mock Slack Web API to assert status round-trips
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![cfg(all(feature = "agui", feature = "channel-slack"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use dravr_canot::agui_consumer::{AgUiConsumer, AgUiEvent};
use dravr_canot::agui_status::{status_text_for_event, StatusAdapter};
use dravr_canot::channels::slack::agui_status::SlackStatusAdapter;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// Shared state for the mock Slack Web API server: every recorded
/// `chat.postMessage` / `chat.update` payload lands in `calls`.
#[derive(Clone)]
struct MockSlackState {
    calls: Arc<Mutex<Vec<Value>>>,
}

/// Start a mock AG-UI SSE server that streams the given event payloads
/// to a single subscriber, then closes the connection.
async fn spawn_mock_sse_server(events: Vec<String>) -> (String, JoinHandle<()>) {
    use axum::extract::Path;
    use axum::response::sse::{Event, KeepAlive, Sse};
    use axum::routing::get;
    use axum::Router;
    use futures_util::stream;

    let events_arc = Arc::new(events);
    let app = Router::new().route(
        "/api/agui/runs/{run_id}/stream",
        get({
            let events = Arc::clone(&events_arc);
            move |Path(_run_id): Path<String>| {
                let events = Arc::clone(&events);
                async move {
                    let items: Vec<Result<Event, Infallible>> = events
                        .iter()
                        .map(|payload| Ok(Event::default().event("agui").data(payload.clone())))
                        .collect();
                    let s = stream::iter(items);
                    Sse::new(s).keep_alive(KeepAlive::default())
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

/// A mock Slack Web API server. Captures every `chat.postMessage` and
/// `chat.update` call into a `Vec` the test can assert against.
///
/// `chat.postMessage` returns a synthetic `ts` so the adapter can
/// target it with subsequent `chat.update` calls.
async fn spawn_mock_slack_api() -> (String, Arc<Mutex<Vec<Value>>>, JoinHandle<()>) {
    use axum::extract::State;
    use axum::routing::post;
    use axum::{Json, Router};

    let calls: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockSlackState {
        calls: Arc::clone(&calls),
    };

    let app = Router::new()
        .route(
            "/chat.postMessage",
            post(
                |State(state): State<MockSlackState>, Json(body): Json<Value>| async move {
                    state
                        .calls
                        .lock()
                        .expect("mutex")
                        .push(json!({ "method": "chat.postMessage", "body": body }));
                    Json(json!({
                        "ok": true,
                        "ts": "1700000000.000100",
                        "channel": "C1",
                        "message": { "text": body.get("text").cloned().unwrap_or(Value::Null) }
                    }))
                },
            ),
        )
        .route(
            "/chat.update",
            post(
                |State(state): State<MockSlackState>, Json(body): Json<Value>| async move {
                    state
                        .calls
                        .lock()
                        .expect("mutex")
                        .push(json!({ "method": "chat.update", "body": body }));
                    Json(json!({
                        "ok": true,
                        "ts": "1700000000.000100",
                        "channel": "C1",
                        "text": body.get("text").cloned().unwrap_or(Value::Null)
                    }))
                },
            ),
        )
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (format!("http://{addr}"), calls, handle)
}

/// Serialize a canonical event payload the way pierre-server does.
fn payload(event: &Value) -> String {
    serde_json::to_string(event).expect("serialize test event")
}

/// Full cross-layer end-to-end:
///
/// 1. Mock platform serves an AG-UI SSE run with a realistic event sequence.
/// 2. Mock Slack Web API captures outbound HTTP calls.
/// 3. `AgUiConsumer` subscribes to the SSE stream.
/// 4. For each event, the test renders status text and forwards it to
///    `SlackStatusAdapter::set_status`, which issues `chat.update`.
/// 5. On `RUN_FINISHED`, the test calls `finalize("…assistant reply…")`
///    which updates the message in place to the final content.
///
/// Assertions: the mock Web API sees one `chat.postMessage` + one
/// `chat.update` per rendered status + one final `chat.update` with
/// the assistant reply.
#[tokio::test(flavor = "multi_thread")]
async fn agui_to_slack_end_to_end() {
    let events = vec![
        payload(&json!({
            "type": "RUN_STARTED",
            "run_id": "run_xyz",
            "thread_id": "thread_1",
            "timestamp": 1,
        })),
        payload(&json!({
            "type": "STEP_STARTED",
            "run_id": "run_xyz",
            "step_name": "prompt_assembly",
            "timestamp": 2,
        })),
        payload(&json!({
            "type": "STEP_FINISHED",
            "run_id": "run_xyz",
            "step_name": "prompt_assembly",
            "timestamp": 3,
        })),
        payload(&json!({
            "type": "TOOL_CALL_START",
            "run_id": "run_xyz",
            "tool_call_id": "call_1",
            "tool_name": "get_activities",
            "timestamp": 4,
        })),
        payload(&json!({
            "type": "STEP_STARTED",
            "run_id": "run_xyz",
            "step_name": "dispatch",
            "timestamp": 5,
        })),
        payload(&json!({
            "type": "RUN_FINISHED",
            "run_id": "run_xyz",
            "timestamp": 6,
        })),
    ];

    let (sse_base, sse_handle) = spawn_mock_sse_server(events).await;
    let (slack_base, slack_calls, slack_handle) = spawn_mock_slack_api().await;

    // Seed a placeholder "thinking…" message via the mock Web API so
    // the adapter has a `ts` to target. Disable the client-side edit
    // throttle so every rendered event lands as an update.
    let adapter = SlackStatusAdapter::open_with_base(
        "xoxb-fake-token",
        "C1",
        None,
        "thinking…",
        slack_base.clone(),
    )
    .await
    .expect("open placeholder")
    .with_edit_throttle(Duration::ZERO);
    assert_eq!(adapter.ts(), "1700000000.000100");

    // Subscribe to the SSE stream.
    let consumer = AgUiConsumer::new(sse_base.parse().expect("sse base url"), "fake_bearer_token");
    let (tx, mut rx) = mpsc::unbounded_channel::<AgUiEvent>();
    let consumer_handle = tokio::spawn(async move {
        let _ = consumer
            .stream("run_xyz", |event| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(event);
                }
            })
            .await;
    });

    let mut saw_run_finished = false;
    let drain_deadline = Duration::from_secs(3);
    let start = Instant::now();
    while start.elapsed() < drain_deadline {
        if let Ok(Some(event)) = timeout(Duration::from_millis(100), rx.recv()).await {
            if let Some(text) = status_text_for_event(&event) {
                adapter.set_status(&text).await.expect("set_status");
            }
            if matches!(event, AgUiEvent::RunFinished { .. }) {
                saw_run_finished = true;
                break;
            }
        }
    }
    assert!(saw_run_finished, "never observed RUN_FINISHED");

    adapter
        .finalize("Your last run was 5 km at 4:30/km.")
        .await
        .expect("finalize");

    consumer_handle.abort();
    sse_handle.abort();
    slack_handle.abort();

    print_slack_trace(&slack_calls);
    assert_slack_call_sequence(&slack_calls);
}

/// Print every captured Slack Web API call so running the test with
/// `--nocapture` doubles as live documentation.
fn print_slack_trace(slack_calls: &Mutex<Vec<Value>>) {
    let calls = slack_calls.lock().expect("mutex").clone();
    eprintln!("\n── Slack Web API call trace ({} calls) ──", calls.len());
    for (i, call) in calls.iter().enumerate() {
        let method = call["method"].as_str().unwrap_or("?");
        let text = call["body"]["text"].as_str().unwrap_or("");
        eprintln!("  {:>2}. {:<18} text={text:?}", i + 1, method);
    }
    eprintln!("── end trace ──\n");
}

/// Assert the expected sequence: exactly one `chat.postMessage` (the
/// placeholder) plus `chat.update` entries that include the mapped
/// status phrases and end with the finalized reply.
fn assert_slack_call_sequence(slack_calls: &Mutex<Vec<Value>>) {
    let calls = slack_calls.lock().expect("mutex").clone();
    let post_count = calls
        .iter()
        .filter(|c| c["method"] == "chat.postMessage")
        .count();
    assert_eq!(post_count, 1, "one chat.postMessage: {calls:?}");

    let update_texts: Vec<String> = calls
        .iter()
        .filter(|c| c["method"] == "chat.update")
        .map(|c| c["body"]["text"].as_str().unwrap_or_default().to_owned())
        .collect();

    // Updates: one per rendered event (RunStarted, StepStarted(pa),
    // ToolCallStart, StepStarted(dispatch)) + one finalize = 5.
    assert!(
        update_texts.len() >= 5,
        "expected at least 5 updates, got {}: {update_texts:?}",
        update_texts.len()
    );

    assert!(update_texts.iter().any(|t| t == "thinking…"));
    assert!(update_texts.iter().any(|t| t == "reading your question…"));
    assert!(update_texts.iter().any(|t| t == "calling get_activities…"));
    assert!(update_texts.iter().any(|t| t == "generating response…"));
    assert_eq!(
        update_texts.last().map(String::as_str),
        Some("Your last run was 5 km at 4:30/km."),
    );

    // Each `chat.update` must target the `ts` returned by the
    // initial `chat.postMessage` — otherwise the edit would create
    // ghost messages or hit the wrong chat.
    for call in calls.iter().filter(|c| c["method"] == "chat.update") {
        assert_eq!(
            call["body"]["ts"], "1700000000.000100",
            "chat.update targeted wrong ts: {call:?}"
        );
        assert_eq!(
            call["body"]["channel"], "C1",
            "chat.update targeted wrong channel: {call:?}"
        );
    }
}
