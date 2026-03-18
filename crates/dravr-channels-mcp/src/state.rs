// ABOUTME: Shared server state holding channel registry and per-channel configuration
// ABOUTME: Thread-safe via Arc<RwLock> for concurrent access from transport handlers
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::sync::Arc;

use dravr_channels::models::{ChannelConfig, ChannelType};
use dravr_channels::ChannelRegistry;
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
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_registry() {
        let state = ServerState::default();
        assert!(state.registry().is_empty());
    }

    #[test]
    fn config_round_trip() {
        let mut state = ServerState::default();
        assert!(state.get_config(&ChannelType::WhatsApp).is_none());

        let config = ChannelConfig {
            api_key: Some("test-key".to_owned()),
            ..ChannelConfig::default()
        };
        state.set_config(ChannelType::WhatsApp, config);

        let retrieved = state.get_config(&ChannelType::WhatsApp).expect("config");
        assert_eq!(retrieved.api_key.as_deref(), Some("test-key"));
    }

    #[test]
    fn set_config_replaces_existing() {
        let mut state = ServerState::default();

        let config1 = ChannelConfig {
            api_key: Some("key-1".to_owned()),
            ..ChannelConfig::default()
        };
        state.set_config(ChannelType::Slack, config1);

        let config2 = ChannelConfig {
            api_key: Some("key-2".to_owned()),
            ..ChannelConfig::default()
        };
        state.set_config(ChannelType::Slack, config2);

        let retrieved = state.get_config(&ChannelType::Slack).expect("config");
        assert_eq!(retrieved.api_key.as_deref(), Some("key-2"));
    }
}
