// ABOUTME: Cross-channel integration tests for InboundReaction webhook parsing
// ABOUTME: Asserts reaction-capable transports parse captured payload shapes and Meta channels invent none
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
#![cfg(all(
    feature = "channel-telegram",
    feature = "channel-slack",
    feature = "channel-discord",
    feature = "channel-whatsapp",
    feature = "channel-messenger",
))]

use dravr_canot::channel::MessagingChannel;
use dravr_canot::channels::discord::transport::DiscordTransport;
use dravr_canot::channels::discord::DiscordDescriptor;
use dravr_canot::channels::messenger::transport::MessengerTransport;
use dravr_canot::channels::messenger::MessengerDescriptor;
use dravr_canot::channels::slack::transport::SlackTransport;
use dravr_canot::channels::slack::SlackDescriptor;
use dravr_canot::channels::telegram::transport::TelegramTransport;
use dravr_canot::channels::telegram::TelegramChannel;
use dravr_canot::channels::telegram::TelegramDescriptor;
use dravr_canot::channels::whatsapp::transport::WhatsAppTransport;
use dravr_canot::channels::whatsapp::WhatsAppDescriptor;
use dravr_canot::descriptor::ChannelDescriptor;
use dravr_canot::models::{ChannelType, ReactionAction};
use dravr_canot::transport::TransportAdapter;
use http::HeaderMap;
use serde_json::{json, Value};

fn to_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap_or_else(|e| panic!("serialize payload: {e}"))
}

// -------------------------------------------------------------------------
// Telegram
// -------------------------------------------------------------------------

fn telegram_reaction_update(old: &Value, new: &Value) -> Value {
    json!({
        "update_id": 10000,
        "message_reaction": {
            "chat": {"id": -1_002_222, "title": "Harricana Crew", "type": "supergroup"},
            "message_id": 42,
            "user": {"id": 456, "is_bot": false, "first_name": "Chef"},
            "date": 1_719_764_700,
            "old_reaction": old,
            "new_reaction": new,
        }
    })
}

#[tokio::test]
async fn telegram_reaction_added_parses_concrete_fields() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = telegram_reaction_update(&json!([]), &json!([{"type": "emoji", "emoji": "👍"}]));
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    let r = &reactions[0];
    assert_eq!(r.channel_type, ChannelType::Telegram);
    assert_eq!(r.channel_message_id, "42");
    assert_eq!(r.reactor_id, "456");
    assert_eq!(r.emoji, "👍");
    assert_eq!(r.action, ReactionAction::Added);
    assert_eq!(r.conversation_id.as_deref(), Some("-1002222"));
}

#[tokio::test]
async fn telegram_reaction_swap_emits_added_and_removed() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = telegram_reaction_update(
        &json!([{"type": "emoji", "emoji": "👍"}]),
        &json!([{"type": "emoji", "emoji": "❤"}]),
    );
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 2);
    assert_eq!(reactions[0].emoji, "❤");
    assert_eq!(reactions[0].action, ReactionAction::Added);
    assert_eq!(reactions[1].emoji, "👍");
    assert_eq!(reactions[1].action, ReactionAction::Removed);
}

#[tokio::test]
async fn telegram_reaction_removal_to_empty_set_emits_removed() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = telegram_reaction_update(&json!([{"type": "emoji", "emoji": "👍"}]), &json!([]));
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].emoji, "👍");
    assert_eq!(reactions[0].action, ReactionAction::Removed);
}

#[tokio::test]
async fn telegram_anonymous_actor_chat_is_the_reactor() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = json!({
        "update_id": 10001,
        "message_reaction": {
            "chat": {"id": -1_002_222, "title": "Harricana Crew", "type": "supergroup"},
            "message_id": 43,
            "actor_chat": {"id": -1_002_222, "title": "Harricana Crew", "type": "supergroup"},
            "date": 1_719_764_701,
            "old_reaction": [],
            "new_reaction": [{"type": "emoji", "emoji": "🔥"}],
        }
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].reactor_id, "-1002222");
}

#[tokio::test]
async fn telegram_custom_emoji_is_named_by_its_id() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = telegram_reaction_update(
        &json!([]),
        &json!([{"type": "custom_emoji", "custom_emoji_id": "5368324170671202286"}]),
    );
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].emoji, "5368324170671202286");
}

#[tokio::test]
async fn telegram_plain_message_update_parses_no_reactions() {
    let transport = TelegramTransport::new("secret".to_owned());
    let update = json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "chat": {"id": 123, "type": "private"},
            "from": {"id": 456, "first_name": "Chef"},
            "text": "hello",
        }
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

#[tokio::test]
async fn telegram_channel_receive_reactions_delegates_to_transport() {
    let channel = TelegramChannel::new("secret".to_owned());
    let update = telegram_reaction_update(&json!([]), &json!([{"type": "emoji", "emoji": "👍"}]));
    let reactions = channel
        .receive_reactions(&HeaderMap::new(), &to_bytes(&update))
        .await
        .unwrap_or_else(|e| panic!("receive_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].channel_message_id, "42");
    assert_eq!(reactions[0].action, ReactionAction::Added);
}

// -------------------------------------------------------------------------
// Slack
// -------------------------------------------------------------------------

fn slack_reaction_event(event_type: &str, item: &Value) -> Value {
    json!({
        "token": "z26uFbvR1xHJEdHE1OQiO6t8",
        "team_id": "T024BE7LD",
        "api_app_id": "A0FFV41KK",
        "type": "event_callback",
        "event_id": "Ev0PV52K21",
        "event_time": 1_360_782_804,
        "event": {
            "type": event_type,
            "user": "U024BE7LH",
            "reaction": "thumbsup",
            "item_user": "U0G9QF9C6",
            "item": item,
            "event_ts": "1360782804.083113",
        }
    })
}

#[tokio::test]
async fn slack_reaction_added_parses_concrete_fields() {
    let transport = SlackTransport::new("secret".to_owned());
    let payload = slack_reaction_event(
        "reaction_added",
        &json!({"type": "message", "channel": "C0G9QF9GZ", "ts": "1360782400.498405"}),
    );
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    let r = &reactions[0];
    assert_eq!(r.channel_type, ChannelType::Slack);
    assert_eq!(r.channel_message_id, "1360782400.498405");
    assert_eq!(r.reactor_id, "U024BE7LH");
    assert_eq!(r.emoji, "thumbsup");
    assert_eq!(r.action, ReactionAction::Added);
    assert_eq!(r.conversation_id.as_deref(), Some("C0G9QF9GZ"));
}

#[tokio::test]
async fn slack_reaction_removed_parses_removed_action() {
    let transport = SlackTransport::new("secret".to_owned());
    let payload = slack_reaction_event(
        "reaction_removed",
        &json!({"type": "message", "channel": "C0G9QF9GZ", "ts": "1360782400.498405"}),
    );
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].action, ReactionAction::Removed);
}

#[tokio::test]
async fn slack_reaction_on_file_item_parses_no_reactions() {
    let transport = SlackTransport::new("secret".to_owned());
    let payload = slack_reaction_event(
        "reaction_added",
        &json!({"type": "file", "file": "F0G9QF9GZ"}),
    );
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

#[tokio::test]
async fn slack_message_event_parses_no_reactions() {
    let transport = SlackTransport::new("secret".to_owned());
    let payload = json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "user": "U024BE7LH",
            "text": "hello",
            "channel": "C0G9QF9GZ",
            "ts": "1360782400.498405",
        }
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

// -------------------------------------------------------------------------
// Discord
// -------------------------------------------------------------------------

fn discord_reaction_dispatch(event_name: &str) -> Value {
    json!({
        "op": 0,
        "s": 7,
        "t": event_name,
        "d": {
            "user_id": "80351110224678912",
            "channel_id": "103735883630395392",
            "message_id": "163361896287969280",
            "guild_id": "103735883630395392",
            "emoji": {"id": null, "name": "🔥"},
        }
    })
}

#[tokio::test]
async fn discord_reaction_add_dispatch_parses_concrete_fields() {
    let transport = DiscordTransport::new("ab".repeat(32), "app-id".to_owned());
    let reactions = transport
        .parse_reactions(
            &HeaderMap::new(),
            &to_bytes(&discord_reaction_dispatch("MESSAGE_REACTION_ADD")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    let r = &reactions[0];
    assert_eq!(r.channel_type, ChannelType::Discord);
    assert_eq!(r.channel_message_id, "163361896287969280");
    assert_eq!(r.reactor_id, "80351110224678912");
    assert_eq!(r.emoji, "🔥");
    assert_eq!(r.action, ReactionAction::Added);
    assert_eq!(r.conversation_id.as_deref(), Some("103735883630395392"));
}

#[tokio::test]
async fn discord_reaction_remove_dispatch_parses_removed_action() {
    let transport = DiscordTransport::new("ab".repeat(32), "app-id".to_owned());
    let reactions = transport
        .parse_reactions(
            &HeaderMap::new(),
            &to_bytes(&discord_reaction_dispatch("MESSAGE_REACTION_REMOVE")),
        )
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].action, ReactionAction::Removed);
}

#[tokio::test]
async fn discord_custom_emoji_falls_back_to_id_when_name_is_null() {
    let transport = DiscordTransport::new("ab".repeat(32), "app-id".to_owned());
    let dispatch = json!({
        "op": 0,
        "s": 8,
        "t": "MESSAGE_REACTION_ADD",
        "d": {
            "user_id": "80351110224678912",
            "channel_id": "103735883630395392",
            "message_id": "163361896287969280",
            "emoji": {"id": "41771983429993937", "name": null},
        }
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&dispatch))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].emoji, "41771983429993937");
}

#[tokio::test]
async fn discord_interaction_payload_parses_no_reactions() {
    let transport = DiscordTransport::new("ab".repeat(32), "app-id".to_owned());
    let interaction = json!({
        "type": 2,
        "id": "846462639134605312",
        "channel_id": "103735883630395392",
        "member": {"user": {"id": "80351110224678912", "username": "chef"}},
        "data": {"name": "coach", "options": [{"name": "text", "value": "hello"}]},
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&interaction))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

// -------------------------------------------------------------------------
// Meta channels: no reaction webhook in this shape — nothing is invented
// -------------------------------------------------------------------------

#[tokio::test]
async fn whatsapp_reaction_shaped_payload_parses_no_reactions() {
    // The WhatsApp Business Cloud API reports reactions as a `messages`
    // entry of type "reaction" — not the reaction-event shape this crate
    // models — so the transport must not fabricate an InboundReaction.
    let transport = WhatsAppTransport::new("secret".to_owned());
    let payload = json!({
        "object": "whatsapp_business_account",
        "entry": [{"id": "1", "changes": [{"field": "messages", "value": {
            "messaging_product": "whatsapp",
            "messages": [{
                "from": "15551234567",
                "id": "wamid.REACTION",
                "timestamp": "1719764700",
                "type": "reaction",
                "reaction": {"message_id": "wamid.TARGET", "emoji": "👍"},
            }],
        }}]}]
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

#[tokio::test]
async fn messenger_reaction_shaped_payload_parses_no_reactions() {
    // Messenger's `message_reactions` webhook field is not the reaction
    // event shape this crate models — the transport must not fabricate
    // an InboundReaction from it.
    let transport = MessengerTransport::new("secret".to_owned());
    let payload = json!({
        "object": "page",
        "entry": [{"id": "1", "time": 1_719_764_700, "messaging": [{
            "sender": {"id": "999"},
            "recipient": {"id": "111"},
            "timestamp": 1_719_764_700,
            "reaction": {"mid": "m_TARGET", "action": "react", "emoji": "👍", "reaction": "like"},
        }]}]
    });
    let reactions = transport
        .parse_reactions(&HeaderMap::new(), &to_bytes(&payload))
        .await
        .unwrap_or_else(|e| panic!("parse_reactions: {e}"));
    assert!(reactions.is_empty());
}

// -------------------------------------------------------------------------
// Descriptor capability
// -------------------------------------------------------------------------

#[test]
fn reaction_capable_channels_advertise_the_capability() {
    assert!(TelegramDescriptor.delivers_inbound_reactions());
    assert!(SlackDescriptor.delivers_inbound_reactions());
    assert!(DiscordDescriptor.delivers_inbound_reactions());
}

#[test]
fn meta_channels_do_not_advertise_inbound_reactions() {
    assert!(!WhatsAppDescriptor.delivers_inbound_reactions());
    assert!(!MessengerDescriptor.delivers_inbound_reactions());
}
