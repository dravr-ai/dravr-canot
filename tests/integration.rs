// ABOUTME: Integration tests for dravr-canot core library
// ABOUTME: Tests registry, factory, models, and channel trait interactions

use dravr_canot::error::MessagingError;
use dravr_canot::models::{
    CardAction, ChannelConfig, ChannelType, DeliveryStatus, MessageContent, OutgoingMessage,
    WebhookTimestampPolicy, RETRY_DELAYS_SECS,
};
use dravr_canot::registry::ChannelRegistry;
use std::str::FromStr;

// ============================================================================
// ChannelType parsing
// ============================================================================

#[test]
fn channel_type_round_trip() {
    for (input, expected) in [
        ("whatsapp", ChannelType::WhatsApp),
        ("messenger", ChannelType::Messenger),
        ("discord", ChannelType::Discord),
        ("slack", ChannelType::Slack),
        ("telegram", ChannelType::Telegram),
    ] {
        let parsed = ChannelType::from_str(input).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), input);
    }
}

#[test]
fn channel_type_case_insensitive() {
    assert_eq!(ChannelType::from_str("SLACK").unwrap(), ChannelType::Slack);
    assert_eq!(
        ChannelType::from_str("WhatsApp").unwrap(),
        ChannelType::WhatsApp
    );
}

#[test]
fn channel_type_unknown_returns_error() {
    assert!(ChannelType::from_str("sms").is_err());
}

// ============================================================================
// MessageContent serialization
// ============================================================================

#[test]
fn text_content_serializes_with_type_tag() {
    let content = MessageContent::Text {
        body: "hello".into(),
    };
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["body"], "hello");
}

#[test]
fn media_content_serializes_with_type_tag() {
    let content = MessageContent::Media {
        url: "https://example.com/img.jpg".into(),
        mime_type: "image/jpeg".into(),
        caption: Some("Photo".into()),
    };
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(json["type"], "media");
    assert_eq!(json["mime_type"], "image/jpeg");
}

#[test]
fn location_content_round_trips() {
    let content = MessageContent::Location {
        latitude: 48.8566,
        longitude: 2.3522,
    };
    let json = serde_json::to_string(&content).unwrap();
    let parsed: MessageContent = serde_json::from_str(&json).unwrap();
    if let MessageContent::Location {
        latitude,
        longitude,
    } = parsed
    {
        assert!((latitude - 48.8566).abs() < f64::EPSILON);
        assert!((longitude - 2.3522).abs() < f64::EPSILON);
    } else {
        panic!("expected Location");
    }
}

#[test]
fn card_content_with_actions() {
    let content = MessageContent::Card {
        title: "Choose".into(),
        body: "Pick one".into(),
        actions: vec![
            CardAction {
                label: "Yes".into(),
                action_type: "postback".into(),
                value: "yes".into(),
            },
            CardAction {
                label: "Visit".into(),
                action_type: "url".into(),
                value: "https://example.com".into(),
            },
        ],
    };
    let json = serde_json::to_value(&content).unwrap();
    assert_eq!(json["type"], "card");
    assert_eq!(json["actions"].as_array().unwrap().len(), 2);
}

// ============================================================================
// ChannelConfig defaults
// ============================================================================

#[test]
fn channel_config_default_is_active() {
    let config: ChannelConfig = serde_json::from_str("{}").unwrap();
    assert!(config.is_active);
    assert!(config.api_key.is_none());
}

// ============================================================================
// DeliveryStatus serialization
// ============================================================================

#[test]
fn delivery_status_lowercase_serde() {
    let status = DeliveryStatus::Delivered;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"delivered\"");
    let parsed: DeliveryStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, DeliveryStatus::Delivered);
}

// ============================================================================
// Registry
// ============================================================================

#[test]
fn empty_registry() {
    let registry = ChannelRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.get(&ChannelType::Slack).is_none());
    assert!(registry.registered_channels().is_empty());
}

// ============================================================================
// Factory — missing config
// ============================================================================

#[cfg(feature = "channel-slack")]
#[test]
fn factory_missing_secret_returns_not_configured() {
    use dravr_canot::factory::create_adapter_from_config;
    let result = create_adapter_from_config(ChannelType::Slack, &serde_json::json!({}));
    match result {
        Err(MessagingError::ChannelNotConfigured { .. }) => {}
        Err(other) => panic!("expected ChannelNotConfigured, got: {other}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[cfg(feature = "channel-slack")]
#[test]
fn factory_creates_slack_adapter() {
    use dravr_canot::factory::create_adapter_from_config;
    let config = serde_json::json!({ "webhook_secret": "test-secret" });
    let adapter = create_adapter_from_config(ChannelType::Slack, &config).unwrap();
    assert_eq!(adapter.channel_type(), ChannelType::Slack);
}

// ============================================================================
// Error retryability
// ============================================================================

#[test]
fn retryable_errors() {
    let err = MessagingError::DeliveryFailed {
        channel: "slack".into(),
        reason: "timeout".into(),
        retryable: true,
    };
    assert!(err.is_retryable());

    let err = MessagingError::ChannelApiError {
        channel: "slack".into(),
        status_code: 429,
        message: "rate limited".into(),
    };
    assert!(err.is_retryable());

    let err = MessagingError::ChannelApiError {
        channel: "slack".into(),
        status_code: 400,
        message: "bad request".into(),
    };
    assert!(!err.is_retryable());
}

// ============================================================================
// Constants
// ============================================================================

#[test]
fn retry_constants_are_sane() {
    assert_eq!(RETRY_DELAYS_SECS.len(), 3);
    assert!(RETRY_DELAYS_SECS[0] < RETRY_DELAYS_SECS[1]);
    assert!(RETRY_DELAYS_SECS[1] < RETRY_DELAYS_SECS[2]);
}

#[test]
fn default_timestamp_policy() {
    let policy = WebhookTimestampPolicy::default();
    assert_eq!(policy.max_age_secs, 300);
}

// ============================================================================
// OutgoingMessage serialization
// ============================================================================

#[test]
fn outgoing_message_round_trips() {
    let msg = OutgoingMessage {
        channel_type: ChannelType::Telegram,
        recipient_id: "12345".into(),
        content: MessageContent::Text {
            body: "test".into(),
        },
        correlation_id: uuid::Uuid::nil(),
        reply_to: Some("msg-001".into()),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: OutgoingMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.channel_type, ChannelType::Telegram);
    assert_eq!(parsed.recipient_id, "12345");
    assert_eq!(parsed.reply_to.as_deref(), Some("msg-001"));
}
