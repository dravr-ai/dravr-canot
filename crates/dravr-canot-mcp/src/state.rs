// ABOUTME: Shared server state holding channel registry and per-channel configuration
// ABOUTME: Thread-safe via Arc<RwLock> for concurrent access from transport handlers
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::sync::Arc;

use dravr_canot::models::{ChannelConfig, ChannelType};
use dravr_canot::ChannelRegistry;
use tokio::sync::RwLock;

/// Type alias for the shared state handle used across the server.
///
/// `dravr-tronc` 0.5 stores the server state as `Arc<S>` and hands tools a shared
/// reference, so mutability lives inside `ServerState` (see `configs`) rather than
/// behind an outer `RwLock`.
pub type SharedState = Arc<ServerState>;

/// Central server state holding registered channels and their configurations
///
/// The channel registry maps `ChannelType` to adapter instances, while the
/// config map stores per-channel API credentials and settings.
pub struct ServerState {
    registry: ChannelRegistry,
    configs: RwLock<HashMap<ChannelType, ChannelConfig>>,
}

impl ServerState {
    /// Create server state with the given channel registry and no configurations
    pub fn new(registry: ChannelRegistry) -> Self {
        Self {
            registry,
            configs: RwLock::new(HashMap::new()),
        }
    }

    /// Get a reference to the channel registry
    pub const fn registry(&self) -> &ChannelRegistry {
        &self.registry
    }

    /// Get a mutable reference to the channel registry for registration
    pub const fn registry_mut(&mut self) -> &mut ChannelRegistry {
        &mut self.registry
    }

    /// Look up configuration for a specific channel type, returning an owned copy
    ///
    /// Reads through the interior `RwLock`: `dravr-tronc` 0.5 hands tools a shared
    /// `&Arc<ServerState>`, so per-field interior mutability replaces the outer lock.
    pub async fn get_config(&self, channel_type: &ChannelType) -> Option<ChannelConfig> {
        self.configs.read().await.get(channel_type).cloned()
    }

    /// Set or replace configuration for a specific channel type
    pub async fn set_config(&self, channel_type: ChannelType, config: ChannelConfig) {
        self.configs.write().await.insert(channel_type, config);
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new(ChannelRegistry::new())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::str_to_string
)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_registry() {
        let state = ServerState::default();
        assert!(state.registry().is_empty());
    }

    fn test_config(channel_type: ChannelType, api_key: &str) -> ChannelConfig {
        ChannelConfig {
            id: "test-id".to_owned(),
            tenant_id: "test-tenant".to_owned(),
            channel_type,
            api_key: Some(api_key.to_owned()),
            api_secret: None,
            webhook_secret: None,
            verify_token: None,
            account_id: None,
            phone_number: None,
            bot_token: None,
            is_active: true,
        }
    }

    #[tokio::test]
    async fn config_round_trip() {
        let state = ServerState::default();
        assert!(state.get_config(&ChannelType::WhatsApp).await.is_none());

        state
            .set_config(
                ChannelType::WhatsApp,
                test_config(ChannelType::WhatsApp, "test-key"),
            )
            .await;

        let retrieved = state
            .get_config(&ChannelType::WhatsApp)
            .await
            .expect("config"); // Safe: just inserted above
        assert_eq!(retrieved.api_key.as_deref(), Some("test-key"));
    }

    #[tokio::test]
    async fn set_config_replaces_existing() {
        let state = ServerState::default();

        state
            .set_config(ChannelType::Slack, test_config(ChannelType::Slack, "key-1"))
            .await;
        state
            .set_config(ChannelType::Slack, test_config(ChannelType::Slack, "key-2"))
            .await;

        let retrieved = state.get_config(&ChannelType::Slack).await.expect("config"); // Safe: just inserted above
        assert_eq!(retrieved.api_key.as_deref(), Some("key-2"));
    }
}
