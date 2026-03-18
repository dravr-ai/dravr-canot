// ABOUTME: Shared static HTTP client for outbound API calls to channel platforms
// ABOUTME: Lazy-initialized with configurable timeouts and connection pooling
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Client;
use tracing::warn;

/// Default request timeout for channel API calls (30 seconds)
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default connection timeout for channel API calls (10 seconds)
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Global shared HTTP client instance
static API_CLIENT: OnceLock<Client> = OnceLock::new();

/// Returns a reference to the shared HTTP client for channel API calls
///
/// The client is lazily initialized on first access with connection pooling,
/// configurable timeouts, and rustls TLS. Subsequent calls return the same
/// client instance without allocation.
pub fn api_client() -> &'static Client {
    API_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to build HTTP client, using default");
                Client::new()
            })
    })
}
