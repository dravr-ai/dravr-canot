// ABOUTME: Shared server state holding channel registry and per-channel configurations
// ABOUTME: Thread-safe via Arc<RwLock> with runtime channel config management
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

/// Central server state tracking channel registry and configurations
///
/// Holds the channel adapter registry for message dispatch and a map of
/// per-channel configurations for API credentials and webhook secrets.
pub struct ServerState {
    registry: ChannelRegistry,
    configs: HashMap<ChannelType, ChannelConfig>,
}

impl ServerState {
    /// Create server state with the given channel registry and empty configs
    pub fn new(registry: ChannelRegistry) -> Self {
        Self {
            registry,
            configs: HashMap::new(),
        }
    }

    /// Get a reference to the channel registry
    pub fn registry(&self) -> &ChannelRegistry {
        &self.registry
    }

    /// Look up the configuration for a specific channel type
    pub fn get_config(&self, channel_type: &ChannelType) -> Option<&ChannelConfig> {
        self.configs.get(channel_type)
    }

    /// Set or replace the configuration for a channel type
    pub fn set_config(&mut self, channel_type: ChannelType, config: ChannelConfig) {
        self.configs.insert(channel_type, config);
    }
}
