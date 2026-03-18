// ABOUTME: Trait for pluggable channel configuration storage backends
// ABOUTME: Enables database-backed, env-var-based, or custom config providers
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use async_trait::async_trait;

use crate::models::{ChannelConfig, ChannelType};

/// Pluggable storage backend for channel configurations
///
/// Implement this trait to provide channel configs from any source:
/// - Environment variables ([`EnvConfigStore`](crate::env_config_store::EnvConfigStore))
/// - Database (implement in your platform crate)
/// - Remote config service, file-based config, etc.
///
/// # Integration Pattern
///
/// This follows the same pattern as `embacle::LlmProvider` — the trait lives
/// in the canot crate, consumers implement it for their storage backend.
///
/// ```ignore
/// // Standalone: read from env vars
/// let store = EnvConfigStore::from_env();
///
/// // Platform: implement the trait on your database layer
/// impl ChannelConfigStore for MyDatabase { ... }
/// ```
#[async_trait]
pub trait ChannelConfigStore: Send + Sync {
    /// Look up channel config by channel type (single-tenant / standalone mode)
    async fn get_config(&self, channel_type: ChannelType) -> Option<ChannelConfig>;

    /// Look up channel config by tenant and channel type (multi-tenant mode)
    ///
    /// Default implementation ignores `tenant_id` and delegates to [`get_config`](Self::get_config).
    /// Multi-tenant consumers should override this.
    async fn get_tenant_config(
        &self,
        _tenant_id: &str,
        channel_type: ChannelType,
    ) -> Option<ChannelConfig> {
        self.get_config(channel_type).await
    }

    /// List all configured channel types
    async fn list_configured_channels(&self) -> Vec<ChannelType>;
}
