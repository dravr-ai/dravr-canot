// ABOUTME: End-to-end test of AG-UI SSE consumption driving Discord PATCH messages status edits
// ABOUTME: Stands up mock SSE source and mock Discord REST API to assert status round-trips
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![cfg(all(feature = "agui", feature = "channel-discord"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(missing_docs)]

use dravr_canot::agui_consumer::{AgUiConsumer, AgUiEvent};
use dravr_canot::agui_status::{status_text_for_event, StatusAdapter};
use dravr_canot::channels::discord::agui_status::DiscordStatusAdapter;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// Shared state for the mock Discord REST API server.
#[derive(Clone)]
struct MockDiscordState {
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

/// A mock Discord REST API server. Captures every POST and PATCH to
/// the `channels/.../messages` endpoints.
async fn spawn_mock_discord_api() -> (String, Arc<Mutex<Vec<Value>>>, JoinHandle<()>) {
    use axum::extract::{Path, State};
    use axum::routing::{patch, post};
    use axum::{Json, Router};

    let calls: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = MockDiscordState {
        calls: Arc::clone(&calls),
    };

    let app = Router::new()
        .route(
            "/channels/{channel_id}/messages",
            post(
                |Path(channel_id): Path<String>,
                 State(state): State<MockDiscordState>,
                 Json(body): Json<Value>| async move {
                    state.calls.lock().expect("mutex").push(json!({
                        "method": "POST messages",
                        "channel_id": channel_id,
                        "body": body,
                    }));
                    Json(json!({
                        "id": "msg_1",
                        "channel_id": channel_id,
                        "content": body.get("content").cloned().unwrap_or(Value::Null),
                    }))
                },
            ),
        )
        .route(
            "/channels/{channel_id}/messages/{message_id}",
            patch(
                |Path((channel_id, message_id)): Path<(String, String)>,
                 State(state): State<MockDiscordState>,
                 Json(body): Json<Value>| async move {
                    state.calls.lock().expect("mutex").push(json!({
                        "method": "PATCH messages",
                        "channel_id": channel_id,
                        "message_id": message_id,
                        "body": body,
                    }));
                    Json(json!({
                        "id": message_id,
                        "channel_id": channel_id,
                        "content": body.get("content").cloned().unwrap_or(Value::Null),
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

fn payload(event: &Value) -> String {
    serde_json::to_string(event).expect("serialize test event")
}

/// Full cross-layer end-to-end:
///
/// 1. Mock platform serves an AG-UI SSE run with a realistic event sequence.
/// 2. Mock Discord REST API captures outbound HTTP calls.
/// 3. `AgUiConsumer` subscribes to the SSE stream.
/// 4. For each event, the test renders status text and forwards it to
///    `DiscordStatusAdapter::set_status`, which issues PATCH messages.
/// 5. On `RUN_FINISHED`, the test calls `finalize("…assistant reply…")`
///    which edits the message in place to the final content.
///
/// Assertions: the mock REST API sees one `POST messages` (the
/// placeholder) + one `PATCH messages` per rendered status + one
/// final `PATCH messages` with the assistant reply.
#[tokio::test(flavor = "multi_thread")]
async fn agui_to_discord_end_to_end() {
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
    let (discord_base, discord_calls, discord_handle) = spawn_mock_discord_api().await;

    let adapter =
        DiscordStatusAdapter::open_with_base("fake_token", "chan_1", "thinking…", discord_base)
            .await
            .expect("open placeholder")
            .with_edit_throttle(Duration::ZERO);
    assert_eq!(adapter.message_id(), "msg_1");

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
    discord_handle.abort();

    print_discord_trace(&discord_calls);
    assert_discord_call_sequence(&discord_calls);
}

fn print_discord_trace(discord_calls: &Mutex<Vec<Value>>) {
    let calls = discord_calls.lock().expect("mutex").clone();
    eprintln!(
        "\n── Discord REST API call trace ({} calls) ──",
        calls.len()
    );
    for (i, call) in calls.iter().enumerate() {
        let method = call["method"].as_str().unwrap_or("?");
        let text = call["body"]["content"].as_str().unwrap_or("");
        eprintln!("  {:>2}. {:<18} content={text:?}", i + 1, method);
    }
    eprintln!("── end trace ──\n");
}

fn assert_discord_call_sequence(discord_calls: &Mutex<Vec<Value>>) {
    let calls = discord_calls.lock().expect("mutex").clone();
    let post_count = calls
        .iter()
        .filter(|c| c["method"] == "POST messages")
        .count();
    assert_eq!(post_count, 1, "one POST messages: {calls:?}");

    let patch_contents: Vec<String> = calls
        .iter()
        .filter(|c| c["method"] == "PATCH messages")
        .map(|c| c["body"]["content"].as_str().unwrap_or_default().to_owned())
        .collect();

    // Patches: one per rendered event (RunStarted, StepStarted(pa),
    // ToolCallStart, StepStarted(dispatch)) + one finalize = 5.
    assert!(
        patch_contents.len() >= 5,
        "expected at least 5 patches, got {}: {patch_contents:?}",
        patch_contents.len()
    );

    assert!(patch_contents.iter().any(|t| t == "thinking…"));
    assert!(patch_contents.iter().any(|t| t == "reading your question…"));
    assert!(patch_contents
        .iter()
        .any(|t| t == "calling get_activities…"));
    assert!(patch_contents.iter().any(|t| t == "generating response…"));
    assert_eq!(
        patch_contents.last().map(String::as_str),
        Some("Your last run was 5 km at 4:30/km."),
    );

    // Each PATCH must target the id returned by the initial POST —
    // otherwise the edit would hit the wrong message.
    for call in calls.iter().filter(|c| c["method"] == "PATCH messages") {
        assert_eq!(
            call["message_id"], "msg_1",
            "PATCH targeted wrong message_id: {call:?}"
        );
        assert_eq!(
            call["channel_id"], "chan_1",
            "PATCH targeted wrong channel: {call:?}"
        );
    }
}
