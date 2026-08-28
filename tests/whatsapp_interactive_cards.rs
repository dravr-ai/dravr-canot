// ABOUTME: WhatsApp cards render as native reply buttons or a list menu, not flattened text
// ABOUTME: and a tapped control round-trips its id back through the inbound parser

// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

//! `WhatsApp` was the only channel whose `supports_cards` was false, so every
//! card degraded to a `"Label: value"` text line and the athlete got no
//! tappable control — even though the Cloud API has carried reply buttons and
//! list menus for years.
//!
//! The caps asserted here are Meta's, and Meta rejects the ENTIRE message
//! when one is exceeded rather than trimming, so each is a send-or-fail
//! boundary: 3 reply buttons, 10 list rows, 20-character button titles unique
//! within a message, 24-character row titles, 256/200-character ids.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(feature = "channel-whatsapp")]

use dravr_canot::channels::whatsapp::renderer::WhatsAppRenderer;
use dravr_canot::channels::whatsapp::transport::WhatsAppTransport;
use dravr_canot::models::{CardAction, ChannelType, MessageContent, OutgoingMessage};
use dravr_canot::renderer::ResponseRenderer;
use dravr_canot::transport::TransportAdapter;
use dravr_canot::turn::ConversationTurnId;
use http::HeaderMap;
use serde_json::{json, Value};

fn postback(label: &str, value: &str) -> CardAction {
    CardAction {
        label: label.to_owned(),
        action_type: "postback".to_owned(),
        value: value.to_owned(),
    }
}

fn link(label: &str, url: &str) -> CardAction {
    CardAction {
        label: label.to_owned(),
        action_type: "url".to_owned(),
        value: url.to_owned(),
    }
}

fn card(actions: Vec<CardAction>) -> OutgoingMessage {
    OutgoingMessage {
        channel_type: ChannelType::WhatsApp,
        recipient_id: "16505551234".to_owned(),
        content: MessageContent::Card {
            title: "Tomorrow".to_owned(),
            body: "Your easy run is at 7am.".to_owned(),
            actions,
        },
        turn_id: ConversationTurnId::new(),
        reply_to: None,
        thread_id: None,
    }
}

fn render(actions: Vec<CardAction>) -> Value {
    WhatsAppRenderer
        .render(&card(actions))
        .unwrap_or_else(|e| panic!("render: {e}"))
}

#[test]
fn the_channel_now_declares_card_support() {
    // The platform's capability table reads this predicate to decide whether
    // to send a Card at all. While it was false the renderer below could
    // never run, so flipping it is part of the fix, not a cosmetic edit.
    assert!(ResponseRenderer::supports_cards(&WhatsAppRenderer));
}

#[test]
fn three_postbacks_render_as_native_reply_buttons() {
    let payload = render(vec![
        postback("Looks good", "/confirm"),
        postback("Reschedule", "/plan move"),
        postback("Skip it", "/skip"),
    ]);

    assert_eq!(payload["type"], "interactive");
    assert_eq!(payload["interactive"]["type"], "button");
    assert_eq!(payload["recipient_type"], "individual");

    let buttons = payload["interactive"]["action"]["buttons"]
        .as_array()
        .unwrap_or_else(|| panic!("no buttons: {payload}"));
    assert_eq!(buttons.len(), 3);
    assert_eq!(buttons[0]["type"], "reply");
    assert_eq!(buttons[0]["reply"]["title"], "Looks good");
    // The id carries the postback verbatim and is never displayed.
    assert_eq!(buttons[1]["reply"]["id"], "/plan move");
    assert_eq!(
        payload["interactive"]["body"]["text"],
        "Your easy run is at 7am."
    );
    assert_eq!(payload["interactive"]["header"]["text"], "Tomorrow");
}

#[test]
fn a_fourth_postback_switches_to_the_list_menu() {
    // 4 exceeds the reply-button cap, so the widget must change rather than
    // the fourth action being dropped.
    let payload = render(vec![
        postback("Plan", "/plan"),
        postback("Status", "/status"),
        postback("Group", "/group status"),
        postback("Coach", "/coach"),
    ]);

    assert_eq!(payload["interactive"]["type"], "list");
    let sections = payload["interactive"]["action"]["sections"]
        .as_array()
        .unwrap_or_else(|| panic!("no sections: {payload}"));
    assert_eq!(
        sections.len(),
        1,
        "one section keeps section.title optional"
    );

    let rows = sections[0]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("no rows: {payload}"));
    assert_eq!(rows.len(), 4, "every action must survive as a row");
    assert_eq!(rows[2]["id"], "/group status");
    assert_eq!(rows[2]["title"], "Group");
    // The open-menu button is required for type list and capped at 20 chars.
    assert_eq!(payload["interactive"]["action"]["button"], "Tomorrow");
}

#[test]
fn eleven_postbacks_exceed_every_widget_and_degrade_to_text() {
    let actions: Vec<CardAction> = (0..11)
        .map(|i| postback(&format!("Option {i}"), &format!("/opt{i}")))
        .collect();
    let payload = render(actions);

    assert_eq!(
        payload["type"], "text",
        "no widget holds 11 rows: {payload}"
    );
    let body = payload["text"]["body"].as_str().unwrap_or_default();
    // Degrading must not lose actions — all eleven stay reachable as text.
    assert!(body.contains("Option 10"), "lost an action: {body}");
}

#[test]
fn a_url_action_keeps_the_card_as_autolinked_text() {
    // Reply buttons cannot hold a link, so a card carrying one stays text —
    // where the URL is the point and WhatsApp autolinks it.
    let payload = render(vec![link("Connect Strava", "https://dravr.ai/connect/abc")]);
    assert_eq!(payload["type"], "text");
    let body = payload["text"]["body"].as_str().unwrap_or_default();
    assert!(
        body.contains("https://dravr.ai/connect/abc"),
        "url lost: {body}"
    );
}

#[test]
fn button_titles_are_truncated_and_kept_unique() {
    // Meta rejects the whole message on duplicate titles, and two labels can
    // collide only AFTER truncation — the case a truncate-then-send renderer
    // silently fails on.
    let long_a = "Reschedule tomorrow morning easy run";
    let long_b = "Reschedule tomorrow evening tempo run";
    let payload = render(vec![postback(long_a, "/a"), postback(long_b, "/b")]);

    let buttons = payload["interactive"]["action"]["buttons"]
        .as_array()
        .unwrap_or_else(|| panic!("expected buttons: {payload}"));
    let first = buttons[0]["reply"]["title"].as_str().unwrap_or_default();
    let second = buttons[1]["reply"]["title"].as_str().unwrap_or_default();

    assert!(first.chars().count() <= 20, "title over cap: {first:?}");
    assert!(second.chars().count() <= 20, "title over cap: {second:?}");
    assert_ne!(first, second, "duplicate titles would be rejected by Meta");
    // Ids must survive the title mangling untouched.
    assert_eq!(buttons[0]["reply"]["id"], "/a");
    assert_eq!(buttons[1]["reply"]["id"], "/b");
}

#[test]
fn an_oversized_postback_id_degrades_rather_than_truncating() {
    // A postback value is the text the press stands for. Truncating it would
    // dispatch a DIFFERENT command silently, so text is the honest fallback.
    let payload = render(vec![postback("Go", &"x".repeat(300))]);
    assert_eq!(
        payload["type"], "text",
        "must not send an over-long id: {payload}"
    );
}

#[test]
fn an_empty_label_degrades_rather_than_being_rejected() {
    let payload = render(vec![postback("   ", "/a")]);
    assert_eq!(
        payload["type"], "text",
        "Meta rejects an empty title: {payload}"
    );
}

#[test]
fn a_card_with_no_actions_is_plain_text() {
    let payload = render(vec![]);
    assert_eq!(payload["type"], "text");
}

// -------------------------------------------------------------------------
// Inbound: a tapped control must reach dispatch, or the buttons are dead
// -------------------------------------------------------------------------

async fn inbound_body(interactive: Value) -> String {
    let payload = json!({
        "object": "whatsapp_business_account",
        "entry": [{
            "id": "102290129340398",
            "changes": [{
                "field": "messages",
                "value": {
                    "messaging_product": "whatsapp",
                    "metadata": {"display_phone_number": "15550783881", "phone_number_id": "1065"},
                    "contacts": [{"profile": {"name": "Chef"}, "wa_id": "16505551234"}],
                    "messages": [{
                        "from": "16505551234",
                        "id": "wamid.TEST",
                        "timestamp": "1714510003",
                        "type": "interactive",
                        "interactive": interactive
                    }]
                }
            }]
        }]
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_else(|e| panic!("serialize: {e}"));
    let messages = WhatsAppTransport::new("secret".to_owned())
        .parse_inbound(&HeaderMap::new(), &bytes)
        .await
        .unwrap_or_else(|e| panic!("parse_inbound: {e}"));
    assert_eq!(messages.len(), 1, "expected one parsed message");
    match &messages[0].content {
        MessageContent::Text { body } => body.clone(),
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
async fn a_tapped_reply_button_dispatches_its_id() {
    let body = inbound_body(json!({
        "type": "button_reply",
        "button_reply": {"id": "/plan move", "title": "Reschedule"}
    }))
    .await;
    // The id, not the display title — the title is truncated and localized.
    assert_eq!(body, "/plan move");
}

#[tokio::test]
async fn a_tapped_list_row_dispatches_its_id() {
    let body = inbound_body(json!({
        "type": "list_reply",
        "list_reply": {"id": "/group status", "title": "Group", "description": "Members"}
    }))
    .await;
    assert_eq!(body, "/group status");
}

#[tokio::test]
async fn a_tap_round_trips_the_exact_id_the_renderer_sent() {
    // The pair that matters: whatever the renderer puts in `reply.id` is what
    // dispatch receives. A change to either side alone breaks this.
    let sent = render(vec![postback("Reschedule", "/plan move")]);
    let id = sent["interactive"]["action"]["buttons"][0]["reply"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no id: {sent}"))
        .to_owned();

    let received = inbound_body(json!({
        "type": "button_reply",
        "button_reply": {"id": id.clone(), "title": "Reschedule"}
    }))
    .await;
    assert_eq!(received, id);
}
