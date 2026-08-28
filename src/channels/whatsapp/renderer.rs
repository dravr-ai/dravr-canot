// ABOUTME: WhatsApp response renderer using Meta Cloud API message format
// ABOUTME: Formats OutgoingMessage into WhatsApp Cloud API JSON payloads for delivery
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use crate::error::MessagingResult;
use crate::models::{CardAction, MessageContent, OutgoingMessage};
use crate::rich_text;
use serde_json::{json, Value};
use std::fmt::Write;

use crate::renderer::ResponseRenderer;

/// `WhatsApp` Cloud API renderer
///
/// Formats messages as Meta `WhatsApp` Business Cloud API payloads:
/// - Text: `{"messaging_product":"whatsapp","to":"phone","type":"text","text":{"body":"..."}}`
/// - Media: `{"messaging_product":"whatsapp","to":"phone","type":"image","image":{"link":"url"}}`
/// - Location: rendered as text body
/// - Card: native reply buttons (1-3 postbacks) or a list menu (4-10);
///   a card carrying a `url` action degrades to an autolinked text body
pub struct WhatsAppRenderer;

impl ResponseRenderer for WhatsAppRenderer {
    fn render(&self, msg: &OutgoingMessage) -> MessagingResult<Value> {
        let to = &msg.recipient_id;

        match &msg.content {
            MessageContent::Text { body } => Ok(json!({
                "messaging_product": "whatsapp",
                "to": to,
                "type": "text",
                "text": { "body": body }
            })),
            MessageContent::RichText { body } => {
                let rendered = rich_text::render_whatsapp_text(&rich_text::parse(body));
                Ok(json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "text",
                    "text": { "body": rendered }
                }))
            }
            MessageContent::Media {
                url,
                mime_type,
                caption,
            } => {
                let media_type = infer_media_type(mime_type);
                let mut media_obj = json!({ "link": url });
                if let Some(cap) = caption {
                    media_obj["caption"] = json!(cap);
                }
                Ok(json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": media_type,
                    media_type: media_obj
                }))
            }
            MessageContent::Location {
                latitude,
                longitude,
            } => {
                let body_text = format!("Location: {latitude}, {longitude}");
                Ok(json!({
                    "messaging_product": "whatsapp",
                    "to": to,
                    "type": "text",
                    "text": { "body": body_text }
                }))
            }
            MessageContent::Card {
                title,
                body,
                actions,
            } => Ok(render_card(to, title, body, actions)),
        }
    }

    fn max_message_length(&self) -> usize {
        4096
    }

    fn supports_media(&self) -> bool {
        true
    }

    fn supports_cards(&self) -> bool {
        true
    }
}

/// Cloud API caps for interactive messages. Meta rejects the entire message
/// when one is exceeded rather than trimming, so a card that cannot satisfy
/// every one of them degrades to text instead of being sent and refused.
const MAX_REPLY_BUTTONS: usize = 3;
/// Rows are capped across all sections combined, not per section.
const MAX_LIST_ROWS: usize = 10;
const MAX_BUTTON_TITLE: usize = 20;
const MAX_ROW_TITLE: usize = 24;
const MAX_BUTTON_ID: usize = 256;
/// Deliberately smaller than [`MAX_BUTTON_ID`] — a list row id caps lower.
const MAX_ROW_ID: usize = 200;
const MAX_LIST_OPEN_LABEL: usize = 20;
const MAX_HEADER_TEXT: usize = 60;

/// Render a card as the richest `WhatsApp` payload its actions allow.
///
/// Reply buttons and list rows carry a `reply.id` / `row.id` that the reader
/// never sees and that returns verbatim in the webhook, so a postback action
/// round-trips losslessly. A `url` action has no interactive equivalent that
/// composes with the others — reply buttons cannot hold a link — so any card
/// carrying one stays text, where the URL is the point and `WhatsApp`
/// autolinks it.
fn render_card(to: &str, title: &str, body: &str, actions: &[CardAction]) -> Value {
    if interactive_eligible(actions) {
        let ids_fit = |max: usize| actions.iter().all(|a| a.value.chars().count() <= max);
        // A postback value is the text the press stands for, so truncating an
        // over-long id would silently dispatch a different command. Falling
        // back to text keeps the press honest.
        if actions.len() <= MAX_REPLY_BUTTONS && ids_fit(MAX_BUTTON_ID) {
            return interactive_buttons(to, title, body, actions);
        }
        if actions.len() <= MAX_LIST_ROWS && ids_fit(MAX_ROW_ID) {
            return interactive_list(to, title, body, actions);
        }
    }
    card_as_text(to, title, body, actions)
}

/// Whether every action can become a native control: at least one action, all
/// of them postbacks, and none carrying a label Meta would reject as empty.
fn interactive_eligible(actions: &[CardAction]) -> bool {
    !actions.is_empty()
        && actions
            .iter()
            .all(|a| a.action_type != "url" && !a.label.trim().is_empty())
}

/// Truncate each label to `max` characters, keeping the results distinct.
///
/// Meta requires reply-button titles to be unique within one message and
/// rejects the whole send otherwise, so two labels differing only past the
/// cut must be separated *after* truncation, never before.
fn unique_titles(actions: &[CardAction], max: usize) -> Vec<String> {
    let mut titles: Vec<String> = Vec::with_capacity(actions.len());
    for action in actions {
        let base: String = action.label.trim().chars().take(max).collect();
        let mut candidate = base.clone();
        let mut ordinal = 2_usize;
        while titles.contains(&candidate) {
            let tag = format!(" {ordinal}");
            let keep = max.saturating_sub(tag.chars().count());
            candidate = base.chars().take(keep).chain(tag.chars()).collect();
            ordinal += 1;
        }
        titles.push(candidate);
    }
    titles
}

/// The `body.text` a card sends. Meta rejects an empty body, so a card
/// carrying only a title falls back to it.
fn card_body_text(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        title.to_owned()
    } else {
        body.to_owned()
    }
}

/// Attach an optional text header, which Meta rejects when empty.
fn with_header(interactive: &mut Value, title: &str, body: &str) {
    if title.trim().is_empty() || body.trim().is_empty() {
        return;
    }
    let header: String = title.chars().take(MAX_HEADER_TEXT).collect();
    interactive["header"] = json!({ "type": "text", "text": header });
}

/// One to three postback actions as native reply buttons.
fn interactive_buttons(to: &str, title: &str, body: &str, actions: &[CardAction]) -> Value {
    let buttons: Vec<Value> = unique_titles(actions, MAX_BUTTON_TITLE)
        .into_iter()
        .zip(actions)
        .map(|(label, action)| {
            json!({ "type": "reply", "reply": { "id": action.value, "title": label } })
        })
        .collect();

    let mut interactive = json!({
        "type": "button",
        "body": { "text": card_body_text(title, body) },
        "action": { "buttons": buttons }
    });
    with_header(&mut interactive, title, body);

    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": to,
        "type": "interactive",
        "interactive": interactive
    })
}

/// Four to ten postback actions as a single-section list — the Cloud API's
/// menu widget, opened by a labelled button.
fn interactive_list(to: &str, title: &str, body: &str, actions: &[CardAction]) -> Value {
    let rows: Vec<Value> = unique_titles(actions, MAX_ROW_TITLE)
        .into_iter()
        .zip(actions)
        .map(|(label, action)| json!({ "id": action.value, "title": label }))
        .collect();

    // One section, so `section.title` stays optional and no invented grouping
    // label reaches the athlete.
    let open_label: String = if title.trim().is_empty() {
        "Menu".to_owned()
    } else {
        title.chars().take(MAX_LIST_OPEN_LABEL).collect()
    };

    let mut interactive = json!({
        "type": "list",
        "body": { "text": card_body_text(title, body) },
        "action": { "button": open_label, "sections": [{ "rows": rows }] }
    });
    with_header(&mut interactive, title, body);

    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": to,
        "type": "interactive",
        "interactive": interactive
    })
}

/// The text degradation for a card no interactive widget can carry.
///
/// A `url` action's value is the link the athlete is meant to follow and a
/// postback's is the text the press stands for, so both are shown: neither is
/// an opaque token, and hiding them would leave an unusable message.
fn card_as_text(to: &str, title: &str, body: &str, actions: &[CardAction]) -> Value {
    let mut text = format!("*{title}*\n\n{body}");
    for action in actions {
        let _ = write!(text, "\n- {}: {}", action.label, action.value);
    }
    json!({
        "messaging_product": "whatsapp",
        "to": to,
        "type": "text",
        "text": { "body": text }
    })
}

/// Map a MIME type to a `WhatsApp` Cloud API media type
fn infer_media_type(mime_type: &str) -> &'static str {
    if mime_type.starts_with("image/") {
        "image"
    } else if mime_type.starts_with("video/") {
        "video"
    } else if mime_type.starts_with("audio/") {
        "audio"
    } else {
        "document"
    }
}
