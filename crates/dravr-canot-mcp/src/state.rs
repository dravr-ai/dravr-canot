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

/// Type alias for the shared state handle used across the server
pub type SharedState = Arc<RwLock<ServerState>>;

/// Central server state holding registered channels and their configurations
///
/// The channel registry maps `ChannelType` to adapter instances, while the
/// config map stores per-channel API credentials and settings.
pub struct ServerState {
    registry: ChannelRegistry,
    configs: HashMap<ChannelType, ChannelConfig>,
}

impl ServerState {
    /// Create server state with the given channel registry and no configurations
    pub fn new(registry: ChannelRegistry) -> Self {
        Self {
            registry,
            configs: HashMap::new(),
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

    /// Look up configuration for a specific channel type
    pub fn get_config(&self, channel_type: &ChannelType) -> Option<&ChannelConfig> {
        self.configs.get(channel_type)
    }

    /// Set or replace configuration for a specific channel type
    pub fn set_config(&mut self, channel_type: ChannelType, config: ChannelConfig) {
        self.configs.insert(channel_type, config);
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

    #[test]
    fn config_round_trip() {
        let mut state = ServerState::default();
        assert!(state.get_config(&ChannelType::WhatsApp).is_none());

        state.set_config(
            ChannelType::WhatsApp,
            test_config(ChannelType::WhatsApp, "test-key"),
        );

        let retrieved = state.get_config(&ChannelType::WhatsApp).expect("config"); // Safe: just inserted above
        assert_eq!(retrieved.api_key.as_deref(), Some("test-key"));
    }

    #[test]
    fn set_config_replaces_existing() {
        let mut state = ServerState::default();

        state.set_config(ChannelType::Slack, test_config(ChannelType::Slack, "key-1"));
        state.set_config(ChannelType::Slack, test_config(ChannelType::Slack, "key-2"));

        let retrieved = state.get_config(&ChannelType::Slack).expect("config"); // Safe: just inserted above
        assert_eq!(retrieved.api_key.as_deref(), Some("key-2"));
    }
}
