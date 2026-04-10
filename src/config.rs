// ABOUTME: Runtime messaging configuration loaded from environment variables
// ABOUTME: All values have compile-time defaults overridable via DRAVR_* env vars
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::env;
use std::str::FromStr;

use crate::models;

/// Environment variable for maximum delivery retry attempts
const ENV_MAX_RETRY_ATTEMPTS: &str = "DRAVR_MAX_RETRY_ATTEMPTS";

/// Environment variable for link code TTL in minutes
const ENV_LINK_CODE_TTL_MINUTES: &str = "DRAVR_LINK_CODE_TTL_MINUTES";

/// Environment variable for OTP code TTL in minutes
const ENV_OTP_TTL_MINUTES: &str = "DRAVR_OTP_TTL_MINUTES";

/// Environment variable for maximum OTP verification attempts
const ENV_MAX_OTP_ATTEMPTS: &str = "DRAVR_MAX_OTP_ATTEMPTS";

/// Environment variable for maximum OTP flows per hour per channel user
const ENV_MAX_OTP_FLOWS_PER_HOUR: &str = "DRAVR_MAX_OTP_FLOWS_PER_HOUR";

/// Runtime messaging configuration with values loaded from environment variables
///
/// Each field falls back to the compile-time default in [`models`] if the
/// corresponding `DRAVR_*` environment variable is absent or unparseable.
///
/// # Environment Variables
///
/// | Variable | Default | Description |
/// |----------|---------|-------------|
/// | `DRAVR_MAX_RETRY_ATTEMPTS` | 3 | Maximum delivery retry attempts before dead-lettering |
/// | `DRAVR_LINK_CODE_TTL_MINUTES` | 10 | Minutes before a link verification code expires |
/// | `DRAVR_OTP_TTL_MINUTES` | 10 | Minutes before an OTP code expires |
/// | `DRAVR_MAX_OTP_ATTEMPTS` | 3 | Maximum OTP verification attempts per flow |
/// | `DRAVR_MAX_OTP_FLOWS_PER_HOUR` | 5 | Maximum OTP flows per hour per channel user |
#[derive(Debug, Clone)]
pub struct MessagingConfig {
    /// Maximum number of delivery retry attempts before dead-lettering
    pub max_retry_attempts: i32,
    /// Retry delay schedule in seconds (exponential backoff)
    pub retry_delays_secs: Vec<u64>,
    /// Duration in minutes before a link verification code expires
    pub link_code_ttl_minutes: i64,
    /// OTP code expiration in minutes
    pub otp_ttl_minutes: i64,
    /// Maximum OTP verification attempts before flow is invalidated
    pub max_otp_attempts: i32,
    /// Maximum OTP flows per hour per `channel_user_id` (rate limiting)
    pub max_otp_flows_per_hour: i64,
}

impl MessagingConfig {
    /// Load configuration from environment variables, falling back to compile-time defaults
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            max_retry_attempts: parse_env_or(ENV_MAX_RETRY_ATTEMPTS, models::MAX_RETRY_ATTEMPTS),
            retry_delays_secs: models::RETRY_DELAYS_SECS.to_vec(),
            link_code_ttl_minutes: parse_env_or(
                ENV_LINK_CODE_TTL_MINUTES,
                models::LINK_CODE_TTL_MINUTES,
            ),
            otp_ttl_minutes: parse_env_or(ENV_OTP_TTL_MINUTES, models::OTP_TTL_MINUTES),
            max_otp_attempts: parse_env_or(ENV_MAX_OTP_ATTEMPTS, models::MAX_OTP_ATTEMPTS),
            max_otp_flows_per_hour: parse_env_or(
                ENV_MAX_OTP_FLOWS_PER_HOUR,
                models::MAX_OTP_FLOWS_PER_HOUR,
            ),
        }
    }
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Parse an environment variable as a numeric type, returning `default` if absent or unparseable
fn parse_env_or<T: FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_compile_time_constants() {
        // Clear any env overrides that might exist
        let config = MessagingConfig {
            max_retry_attempts: models::MAX_RETRY_ATTEMPTS,
            retry_delays_secs: models::RETRY_DELAYS_SECS.to_vec(),
            link_code_ttl_minutes: models::LINK_CODE_TTL_MINUTES,
            otp_ttl_minutes: models::OTP_TTL_MINUTES,
            max_otp_attempts: models::MAX_OTP_ATTEMPTS,
            max_otp_flows_per_hour: models::MAX_OTP_FLOWS_PER_HOUR,
        };

        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.retry_delays_secs, vec![1, 5, 30]);
        assert_eq!(config.link_code_ttl_minutes, 10);
        assert_eq!(config.otp_ttl_minutes, 10);
        assert_eq!(config.max_otp_attempts, 3);
        assert_eq!(config.max_otp_flows_per_hour, 5);
    }

    #[test]
    fn parse_env_or_returns_default_for_missing_var() {
        let val: i32 = parse_env_or("DRAVR_NONEXISTENT_TEST_VAR_12345", 42);
        assert_eq!(val, 42);
    }
}
