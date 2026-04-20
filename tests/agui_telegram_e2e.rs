// ABOUTME: End-to-end test of AG-UI SSE consumption driving Telegram editMessageText updates
// ABOUTME: Stands up mock SSE source and mock Telegram Bot API to assert status round-trips
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![cfg(all(feature = "agui", feature = "channel-telegram"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use dravr_canot::agui_consumer::{AgUiConsumer, AgUiEvent};
use dravr_canot::agui_status::{status_text_for_event, StatusAdapter};
use dravr_canot::channels::telegram::agui_status::TelegramStatusAdapter;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// Shared state for the mock Telegram Bot API server: every recorded
/// `sendMessage` / `editMessageText` payload lands in `calls`.
#[derive(Clone)]
struct MockTelegramState {
    calls: Arc<Mutex<Vec<Value>>>,
}

/// Start a mock AG-UI SSE server that streams the given event
/// payloads to a single subscriber, then closes the connection.
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

/// A mock Telegram Bot API server. Captures every `sendMessage` and
/// `editMessageText` call into a `Vec` the test can assert against.
///
/// `sendMessage` returns a synthetic `message_id` so the adapter can
/// target it with subsequent `editMessageText` calls.
async fn spawn_mock_telegram_api() -> (String, Arc<Mutex<Vec<Value>>>, JoinHandle<()>) {
    use axum::extract::{Path, State};
    use axum::routing::post;
    use axum::{Json, Router};

    let calls: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockTelegramState {
        calls: Arc::clone(&calls),
    };

    let app = Router::new()
        .route(
            "/bot{token}/sendMessage",
            post(
                |Path(_token): Path<String>,
                 State(state): State<MockTelegramState>,
                 Json(body): Json<Value>| async move {
                    state
                        .calls
                        .lock()
                        .expect("mutex")
                        .push(json!({ "method": "sendMessage", "body": body }));
                    Json(json!({
                        "ok": true,
                        "result": {
                            "message_id": 4242,
                            "chat": { "id": 1 },
                        }
                    }))
                },
            ),
        )
        .route(
            "/bot{token}/editMessageText",
            post(
                |Path(_token): Path<String>,
                 State(state): State<MockTelegramState>,
                 Json(body): Json<Value>| async move {
                    state
                        .calls
                        .lock()
                        .expect("mutex")
                        .push(json!({ "method": "editMessageText", "body": body }));
                    Json(json!({ "ok": true, "result": { "message_id": 4242 } }))
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

/// Status text mapping covers the common lifecycle events.
#[test]
fn status_text_for_core_events() {
    let e = AgUiEvent::RunStarted {
        run_id: "r".into(),
        thread_id: None,
        timestamp: 1,
    };
    assert!(status_text_for_event(&e).is_some());

    let e = AgUiEvent::StepStarted {
        run_id: "r".into(),
        step_name: "dispatch".into(),
        timestamp: 1,
    };
    assert_eq!(
        status_text_for_event(&e).as_deref(),
        Some("generating response…"),
    );

    // Finish events are deliberately silent — the caller renders the
    // finalized reply via `finalize`, not a transient status update.
    let e = AgUiEvent::StepFinished {
        run_id: "r".into(),
        step_name: "dispatch".into(),
        timestamp: 1,
    };
    assert!(status_text_for_event(&e).is_none());
}

/// Full cross-layer end-to-end:
///
/// 1. Mock platform serves an AG-UI SSE run with a realistic event sequence.
/// 2. Mock Telegram Bot API captures outbound HTTP calls.
/// 3. `AgUiConsumer` subscribes to the SSE stream.
/// 4. For each event, the test renders status text and forwards it to
///    `TelegramStatusAdapter::set_status`, which issues `editMessageText`.
/// 5. On `RUN_FINISHED`, the test calls `finalize("…assistant reply…")`
///    which edits the message in place to the final content.
///
/// Assertions: the mock Bot API sees one `sendMessage` + one
/// `editMessageText` per rendered status + one final `editMessageText`
/// with the assistant reply.
#[tokio::test(flavor = "multi_thread")]
async fn agui_to_telegram_end_to_end() {
    // Event sequence the mock SSE server replays — the same shape the
    // real pierre-server pipeline emits.
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
    let (tg_base, tg_calls, tg_handle) = spawn_mock_telegram_api().await;

    // Seed a placeholder "thinking…" message via the mock Bot API so
    // the adapter has a message_id to target.
    // Disable the client-side edit throttle so every rendered event
    // lands as an edit; the test asserts the full vocabulary mapping,
    // not the throttle (which has its own unit coverage).
    let adapter = TelegramStatusAdapter::open_with_base(
        "fake_token",
        "1", // chat_id
        None,
        "thinking…",
        tg_base.clone(),
    )
    .await
    .expect("open placeholder")
    .with_edit_throttle(std::time::Duration::ZERO);
    assert_eq!(adapter.message_id(), 4242);

    // Subscribe to the SSE stream. Forward every event into a channel
    // so the test can drain them deterministically.
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

    // Drain events and push status updates through the adapter until
    // we observe RUN_FINISHED.
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

    // Finalize with the assistant reply.
    adapter
        .finalize("Your last run was 5 km at 4:30/km.")
        .await
        .expect("finalize");

    // Stop background tasks.
    consumer_handle.abort();
    sse_handle.abort();
    tg_handle.abort();

    // Print the captured Telegram API trace. Run with `--nocapture`
    // to see it; this is the reproducible "proof" that the AG-UI ↔
    // Telegram wiring produces the exact edit-message sequence the
    // README documents.
    print_telegram_trace(&tg_calls);

    assert_telegram_call_sequence(&tg_calls);
}

/// Print every captured Telegram API call from the mock Bot API so
/// running the test with `--nocapture` doubles as live documentation.
fn print_telegram_trace(tg_calls: &Mutex<Vec<Value>>) {
    let calls = tg_calls.lock().expect("mutex").clone();
    eprintln!(
        "\n── Telegram Bot API call trace ({} calls) ──",
        calls.len()
    );
    for (i, call) in calls.iter().enumerate() {
        let method = call["method"].as_str().unwrap_or("?");
        let text = call["body"]["text"].as_str().unwrap_or("");
        eprintln!("  {:>2}. {:<18} text={text:?}", i + 1, method);
    }
    eprintln!("── end trace ──\n");
}

/// Assert the expected sequence of Telegram Bot API calls: exactly one
/// `sendMessage` (the placeholder) plus `editMessageText` entries that
/// include the mapped status phrases and end with the finalized reply.
fn assert_telegram_call_sequence(tg_calls: &Mutex<Vec<Value>>) {
    let calls = tg_calls.lock().expect("mutex").clone();
    let send_count = calls
        .iter()
        .filter(|c| c["method"] == "sendMessage")
        .count();
    assert_eq!(send_count, 1, "one sendMessage: {calls:?}");

    // Edits: one per rendered event (RunStarted, StepStarted(pa),
    // ToolCallStart, StepStarted(dispatch)) + one finalize = 5.
    let edit_texts: Vec<String> = calls
        .iter()
        .filter(|c| c["method"] == "editMessageText")
        .map(|c| c["body"]["text"].as_str().unwrap_or_default().to_owned())
        .collect();

    assert!(
        edit_texts.len() >= 5,
        "expected at least 5 edits, got {}: {edit_texts:?}",
        edit_texts.len()
    );

    assert!(edit_texts.iter().any(|t| t == "thinking…"));
    assert!(edit_texts.iter().any(|t| t == "reading your question…"));
    assert!(edit_texts.iter().any(|t| t == "calling get_activities…"));
    assert!(edit_texts.iter().any(|t| t == "generating response…"));
    assert_eq!(
        edit_texts.last().map(String::as_str),
        Some("Your last run was 5 km at 4:30/km."),
    );
}
