// ABOUTME: Runnable demo — subscribe to a pierre-server AG-UI run and render live Telegram status
// ABOUTME: Usage: cargo run --example telegram_agui_demo --features agui,channel-telegram
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! End-to-end Telegram AG-UI demo.
//!
//! Connects to a live pierre-server AG-UI SSE run, maps each event to a
//! short status line, and updates a single Telegram message in place.
//! The final edit shows the assistant's complete reply; the user only
//! ever sees one message in their chat history regardless of how many
//! pipeline stages the turn went through.
//!
//! # Running the demo
//!
//! ```text
//! export TELEGRAM_BOT_TOKEN="123456:ABC..."     # from @BotFather
//! export TELEGRAM_CHAT_ID="987654321"           # target chat id
//! export PIERRE_BASE_URL="https://pierre.example.com"
//! export PIERRE_BEARER_TOKEN="<user JWT>"
//! export PIERRE_RUN_ID="run_abc"                # run_id returned from chat send_message
//! export PIERRE_FINAL_REPLY="Last run: 5 km at 4:30/km."
//! cargo run --example telegram_agui_demo --features agui,channel-telegram
//! ```
//!
//! The example itself is a 60-line function: open a placeholder
//! message, subscribe, render per-event status, finalize on
//! `RUN_FINISHED`. Every piece of behavior that lands on the wire has
//! matching coverage in `tests/agui_telegram_e2e.rs` — the demo
//! exercises the same code paths against real endpoints.

use dravr_canot::agui_consumer::{AgUiConsumer, AgUiEvent};
use dravr_canot::agui_status::{status_text_for_event, StatusAdapter};
use dravr_canot::channels::telegram::agui_status::TelegramStatusAdapter;
use std::env;
use std::error::Error;
use std::sync::Arc;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    fmt::init();

    let bot_token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN must be set");
    let chat_id = env::var("TELEGRAM_CHAT_ID").expect("TELEGRAM_CHAT_ID must be set");
    let base_url = env::var("PIERRE_BASE_URL").expect("PIERRE_BASE_URL must be set");
    let bearer = env::var("PIERRE_BEARER_TOKEN").expect("PIERRE_BEARER_TOKEN must be set");
    let run_id = env::var("PIERRE_RUN_ID").expect("PIERRE_RUN_ID must be set");
    let final_reply = env::var("PIERRE_FINAL_REPLY").unwrap_or_else(|_| "done".to_owned());

    // 1. Post the initial "thinking…" message and remember its
    //    message_id so all subsequent edits target the same chat line.
    let adapter = TelegramStatusAdapter::open(&bot_token, &chat_id, None, "thinking…").await?;
    let adapter = Arc::new(adapter);
    eprintln!("posted placeholder message_id={}", adapter.message_id());

    // 2. Subscribe to the platform's AG-UI SSE run stream.
    let consumer = AgUiConsumer::new(base_url.parse()?, bearer);

    // 3. For each event, render the canonical status line and edit the
    //    placeholder in place. Finish events pass through `finalize`
    //    below; everything else renders a transient status.
    let adapter_for_stream = Arc::clone(&adapter);
    let run_id_for_stream = run_id.clone();
    consumer
        .stream(&run_id, move |event| {
            let adapter = Arc::clone(&adapter_for_stream);
            let run_id_for_stream = run_id_for_stream.clone();
            async move {
                if matches!(&event, AgUiEvent::RunFinished { .. }) {
                    eprintln!("[{run_id_for_stream}] RUN_FINISHED");
                    return;
                }
                if let AgUiEvent::RunError { code, message, .. } = &event {
                    eprintln!("[{run_id_for_stream}] RUN_ERROR code={code} message={message}");
                    return;
                }
                if let Some(text) = status_text_for_event(&event) {
                    eprintln!("[{run_id_for_stream}] status → {text}");
                    if let Err(e) = adapter.set_status(&text).await {
                        eprintln!("[{run_id_for_stream}] set_status error: {e}");
                    }
                }
            }
        })
        .await?;

    // 4. Replace the status with the final assistant reply. The user
    //    sees the "thinking…" line seamlessly become the answer.
    adapter.finalize(&final_reply).await?;
    eprintln!(
        "finalized with reply ({len} chars)",
        len = final_reply.len()
    );

    Ok(())
}
