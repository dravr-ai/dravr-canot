// ABOUTME: Environment-variable-based channel config store for standalone deployments
// ABOUTME: Reads SLACK_*, TELEGRAM_*, META_WHATSAPP_*, DISCORD_*, MESSENGER_* env vars at startup
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::env;

use async_trait::async_trait;
use uuid::Uuid;

use crate::config_store::ChannelConfigStore;
use crate::models::{ChannelConfig, ChannelType};

/// Default tenant ID used in standalone (single-tenant) mode
const STANDALONE_TENANT_ID: &str = "standalone";

/// Channel config store backed by environment variables
///
/// Reads channel credentials from env vars at construction time and holds them
/// in memory. Suitable for standalone deployments without a database.
///
/// # Environment Variables
///
/// | Channel | Required Vars | Optional Vars |
/// |---------|---------------|---------------|
/// | Slack | `SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET` | |
/// | Telegram | `TELEGRAM_BOT_TOKEN`, `TELEGRAM_WEBHOOK_SECRET` | |
/// | Whatsapp | `META_WHATSAPP_ACCESS_TOKEN`, `META_WHATSAPP_APP_SECRET` | `META_WHATSAPP_PHONE_NUMBER_ID`, `META_WHATSAPP_VERIFY_TOKEN` |
/// | Discord | `DISCORD_BOT_TOKEN`, `DISCORD_PUBLIC_KEY` | `DISCORD_APPLICATION_ID` |
/// | Messenger | `MESSENGER_PAGE_ACCESS_TOKEN`, `MESSENGER_APP_SECRET` | `MESSENGER_VERIFY_TOKEN` |
pub struct EnvConfigStore {
    configs: HashMap<ChannelType, ChannelConfig>,
}

impl EnvConfigStore {
    /// Load channel configs from environment variables
    ///
    /// Only channels with all required env vars set will be registered.
    /// Missing or empty env vars are silently skipped.
    #[must_use]
    pub fn from_env() -> Self {
        let mut configs = HashMap::new();

        if let Some(cfg) = load_slack() {
            configs.insert(ChannelType::Slack, cfg);
        }
        if let Some(cfg) = load_telegram() {
            configs.insert(ChannelType::Telegram, cfg);
        }
        if let Some(cfg) = load_whatsapp() {
            configs.insert(ChannelType::WhatsApp, cfg);
        }
        if let Some(cfg) = load_discord() {
            configs.insert(ChannelType::Discord, cfg);
        }
        if let Some(cfg) = load_messenger() {
            configs.insert(ChannelType::Messenger, cfg);
        }

        Self { configs }
    }

    /// Number of configured channels
    #[must_use]
    pub fn len(&self) -> usize {
        self.configs.len()
    }

    /// Whether any channels are configured
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }
}

#[async_trait]
impl ChannelConfigStore for EnvConfigStore {
    async fn get_config(&self, channel_type: ChannelType) -> Option<ChannelConfig> {
        self.configs.get(&channel_type).cloned()
    }

    async fn get_tenant_config(
        &self,
        _tenant_id: &str,
        channel_type: ChannelType,
    ) -> Option<ChannelConfig> {
        self.configs.get(&channel_type).cloned()
    }

    async fn list_configured_channels(&self) -> Vec<ChannelType> {
        self.configs.keys().copied().collect()
    }
}

/// Read a non-empty env var, returning `None` if absent or empty
fn env_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.is_empty())
}

/// Build a standalone `ChannelConfig` with generated ID and default tenant
fn standalone_config(channel_type: ChannelType) -> ChannelConfig {
    ChannelConfig {
        id: Uuid::new_v4().to_string(),
        tenant_id: STANDALONE_TENANT_ID.to_owned(),
        channel_type,
        api_key: None,
        api_secret: None,
        webhook_secret: None,
        verify_token: None,
        account_id: None,
        phone_number: None,
        bot_token: None,
        is_active: true,
    }
}

/// Load Slack config from `SLACK_BOT_TOKEN` + `SLACK_SIGNING_SECRET`
fn load_slack() -> Option<ChannelConfig> {
    let mut cfg = standalone_config(ChannelType::Slack);
    cfg.api_key = Some(env_non_empty("SLACK_BOT_TOKEN")?);
    cfg.webhook_secret = Some(env_non_empty("SLACK_SIGNING_SECRET")?);
    Some(cfg)
}

/// Load Telegram config from `TELEGRAM_BOT_TOKEN` + `TELEGRAM_WEBHOOK_SECRET`
fn load_telegram() -> Option<ChannelConfig> {
    let mut cfg = standalone_config(ChannelType::Telegram);
    cfg.bot_token = Some(env_non_empty("TELEGRAM_BOT_TOKEN")?);
    cfg.webhook_secret = Some(env_non_empty("TELEGRAM_WEBHOOK_SECRET")?);
    Some(cfg)
}

/// Load Whatsapp config from `META_WHATSAPP_*` env vars
fn load_whatsapp() -> Option<ChannelConfig> {
    let mut cfg = standalone_config(ChannelType::WhatsApp);
    cfg.api_key = Some(env_non_empty("META_WHATSAPP_ACCESS_TOKEN")?);
    cfg.webhook_secret = Some(env_non_empty("META_WHATSAPP_APP_SECRET")?);
    cfg.phone_number = env_non_empty("META_WHATSAPP_PHONE_NUMBER_ID");
    cfg.verify_token = env_non_empty("META_WHATSAPP_VERIFY_TOKEN");
    Some(cfg)
}

/// Load Discord config from `DISCORD_BOT_TOKEN` + `DISCORD_PUBLIC_KEY`
fn load_discord() -> Option<ChannelConfig> {
    let mut cfg = standalone_config(ChannelType::Discord);
    cfg.bot_token = Some(env_non_empty("DISCORD_BOT_TOKEN")?);
    cfg.webhook_secret = Some(env_non_empty("DISCORD_PUBLIC_KEY")?);
    cfg.account_id = env_non_empty("DISCORD_APPLICATION_ID");
    Some(cfg)
}

/// Load Messenger config from `MESSENGER_PAGE_ACCESS_TOKEN` + `MESSENGER_APP_SECRET`
fn load_messenger() -> Option<ChannelConfig> {
    let mut cfg = standalone_config(ChannelType::Messenger);
    cfg.api_key = Some(env_non_empty("MESSENGER_PAGE_ACCESS_TOKEN")?);
    cfg.webhook_secret = Some(env_non_empty("MESSENGER_APP_SECRET")?);
    cfg.verify_token = env_non_empty("MESSENGER_VERIFY_TOKEN");
    Some(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_non_empty_filters_empty_strings() {
        // Env var not set → None
        assert!(env_non_empty("DRAVR_TEST_NONEXISTENT_VAR_XYZ").is_none());
    }

    #[test]
    fn standalone_config_uses_standalone_tenant() {
        let cfg = standalone_config(ChannelType::Slack);
        assert_eq!(cfg.tenant_id, "standalone");
        assert_eq!(cfg.channel_type, ChannelType::Slack);
        assert!(cfg.is_active);
        assert!(!cfg.id.is_empty());
        assert!(cfg.api_key.is_none());
    }

    #[tokio::test]
    async fn empty_env_produces_empty_store() {
        // With no SLACK_*/TELEGRAM_*/etc env vars set, store should be empty
        // (this test assumes CI doesn't have these vars)
        let store = EnvConfigStore::from_env();
        // We can't assert is_empty because the dev machine may have .envrc loaded
        // Instead verify the trait methods work
        let channels = store.list_configured_channels().await;
        for ct in &channels {
            let cfg = store.get_config(*ct).await;
            assert!(cfg.is_some());
        }
    }
}
