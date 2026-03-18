// ABOUTME: Integration tests for dravr-canot core library
// ABOUTME: Tests registry, factory, models, config, and channel trait interactions

use dravr_canot::config::MessagingConfig;
use dravr_canot::error::MessagingError;
use dravr_canot::models::{
    CardAction, ChannelConfig, ChannelType, DeliveryStatus, LinkingMethod, MessageContent,
    MessagingChannelLink, MessagingLinkState, MessagingSession, OutboundQueueEntry,
    OutgoingMessage, WebhookTimestampPolicy, LINK_CODE_TTL_MINUTES, MAX_OTP_ATTEMPTS,
    MAX_OTP_FLOWS_PER_HOUR, OTP_TTL_MINUTES, RETRY_DELAYS_SECS,
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
// Linking method
// ============================================================================

#[test]
fn channel_type_linking_methods() {
    assert_eq!(
        ChannelType::Telegram.linking_method(),
        LinkingMethod::DeepLink
    );
    assert_eq!(
        ChannelType::WhatsApp.linking_method(),
        LinkingMethod::DeepLink
    );
    assert_eq!(ChannelType::Slack.linking_method(), LinkingMethod::OAuth);
    assert_eq!(ChannelType::Discord.linking_method(), LinkingMethod::OAuth);
    assert_eq!(
        ChannelType::Messenger.linking_method(),
        LinkingMethod::OAuth
    );
}

#[test]
fn linking_method_round_trip() {
    assert_eq!(
        LinkingMethod::from_str("deep_link").unwrap(),
        LinkingMethod::DeepLink
    );
    assert_eq!(
        LinkingMethod::from_str("oauth").unwrap(),
        LinkingMethod::OAuth
    );
    assert_eq!(LinkingMethod::DeepLink.to_string(), "deep_link");
    assert_eq!(LinkingMethod::OAuth.to_string(), "oauth");
}

#[test]
fn linking_method_unknown_returns_error() {
    assert!(LinkingMethod::from_str("magic_link").is_err());
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
// ChannelConfig
// ============================================================================

#[test]
fn channel_config_round_trips() {
    let config = ChannelConfig {
        id: "cfg-001".into(),
        tenant_id: "tenant-1".into(),
        channel_type: ChannelType::Slack,
        api_key: Some("xoxb-test".into()),
        api_secret: None,
        webhook_secret: Some("secret".into()),
        verify_token: None,
        account_id: None,
        phone_number: None,
        bot_token: None,
        is_active: true,
    };
    let json = serde_json::to_string(&config).unwrap();
    let parsed: ChannelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "cfg-001");
    assert_eq!(parsed.tenant_id, "tenant-1");
    assert_eq!(parsed.channel_type, ChannelType::Slack);
    assert!(parsed.is_active);
    assert_eq!(parsed.api_key.as_deref(), Some("xoxb-test"));
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

    let err = MessagingError::RateLimitExceeded {
        channel: "slack".into(),
        retry_after_secs: 60,
    };
    assert!(err.is_retryable());
    assert_eq!(err.retry_after_secs(), Some(60));

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

#[test]
fn non_retryable_errors() {
    let err = MessagingError::LinkCodeExpired;
    assert!(!err.is_retryable());
    assert!(err.retry_after_secs().is_none());

    let err = MessagingError::SessionNotFound {
        session_id: "sess-1".into(),
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
fn linking_and_otp_constants() {
    assert_eq!(LINK_CODE_TTL_MINUTES, 10);
    assert_eq!(OTP_TTL_MINUTES, 10);
    assert_eq!(MAX_OTP_ATTEMPTS, 3);
    assert_eq!(MAX_OTP_FLOWS_PER_HOUR, 5);
}

#[test]
fn default_timestamp_policy() {
    let policy = WebhookTimestampPolicy::default();
    assert_eq!(policy.max_age_secs, 300);
}

// ============================================================================
// Config from env
// ============================================================================

#[test]
fn messaging_config_defaults() {
    let config = MessagingConfig::from_env();
    assert_eq!(config.max_retry_attempts, 3);
    assert_eq!(config.retry_delays_secs, vec![1, 5, 30]);
    assert_eq!(config.link_code_ttl_minutes, 10);
    assert_eq!(config.otp_ttl_minutes, 10);
    assert_eq!(config.max_otp_attempts, 3);
    assert_eq!(config.max_otp_flows_per_hour, 5);
}

// ============================================================================
// Session model
// ============================================================================

#[test]
fn messaging_session_round_trips() {
    let session = MessagingSession {
        id: "sess-001".into(),
        user_id: "user-1".into(),
        tenant_id: "tenant-1".into(),
        channel_type: ChannelType::WhatsApp,
        channel_user_id: "+15551234567".into(),
        channel_conversation_id: None,
        conversation_id: Some("conv-abc".into()),
        last_message_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&session).unwrap();
    let parsed: MessagingSession = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "sess-001");
    assert_eq!(parsed.channel_type, ChannelType::WhatsApp);
    assert_eq!(parsed.channel_user_id, "+15551234567");
}

// ============================================================================
// Link state model
// ============================================================================

#[test]
fn messaging_link_state_round_trips() {
    let now = chrono::Utc::now();
    let state = MessagingLinkState {
        id: "link-001".into(),
        tenant_id: "tenant-1".into(),
        user_id: "user-1".into(),
        channel_type: ChannelType::Telegram,
        code: "ABC123".into(),
        method: LinkingMethod::DeepLink,
        used: false,
        expires_at: now,
        created_at: now,
    };
    let json = serde_json::to_string(&state).unwrap();
    let parsed: MessagingLinkState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.code, "ABC123");
    assert_eq!(parsed.method, LinkingMethod::DeepLink);
    assert!(!parsed.used);
}

// ============================================================================
// Channel link model
// ============================================================================

#[test]
fn messaging_channel_link_round_trips() {
    let link = MessagingChannelLink {
        id: "cl-001".into(),
        tenant_id: "tenant-1".into(),
        user_id: "user-1".into(),
        channel_type: ChannelType::Discord,
        channel_user_id: "123456789".into(),
        display_name: Some("TestUser#1234".into()),
        linked_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&link).unwrap();
    let parsed: MessagingChannelLink = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.channel_user_id, "123456789");
    assert_eq!(parsed.display_name.as_deref(), Some("TestUser#1234"));
}

// ============================================================================
// Outbound queue entry
// ============================================================================

#[test]
fn outbound_queue_entry_round_trips() {
    let entry = OutboundQueueEntry {
        id: "q-001".into(),
        message_id: "msg-001".into(),
        tenant_id: "tenant-1".into(),
        channel_type: ChannelType::Slack,
        payload: serde_json::json!({"text": "hello"}),
        status: "pending".into(),
        attempt_count: 0,
        next_retry_at: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    let parsed: OutboundQueueEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "q-001");
    assert_eq!(parsed.attempt_count, 0);
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

// ============================================================================
// ChannelConfigStore trait + EnvConfigStore
// ============================================================================

#[tokio::test]
async fn env_config_store_trait_methods() {
    use dravr_canot::config_store::ChannelConfigStore;
    use dravr_canot::env_config_store::EnvConfigStore;

    let store = EnvConfigStore::from_env();
    let channels = store.list_configured_channels().await;

    // Every listed channel should return a config
    for ct in &channels {
        let cfg = store.get_config(*ct).await;
        assert!(cfg.is_some(), "listed channel {ct} should have a config");

        // tenant_config with any tenant_id should also work (standalone ignores tenant)
        let tenant_cfg = store.get_tenant_config("any-tenant", *ct).await;
        assert!(tenant_cfg.is_some());
    }

    // Unlisted channel types should return None
    // (at least one of these won't be configured in most envs)
    if !channels.contains(&ChannelType::Discord) {
        assert!(store.get_config(ChannelType::Discord).await.is_none());
    }
}
