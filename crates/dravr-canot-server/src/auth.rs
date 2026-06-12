// ABOUTME: Optional bearer token authentication middleware for the REST API
// ABOUTME: Enforces DRAVR_CHANNELS_API_KEY when set, allows unauthenticated access otherwise
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use dravr_tronc::server::auth;

/// Environment variable name for the API key
const API_KEY_ENV: &str = "DRAVR_CHANNELS_API_KEY";

/// Middleware that validates the bearer token against `DRAVR_CHANNELS_API_KEY`
///
/// The env var is read on every request to allow runtime key rotation
/// without restarting the server. If the variable is not set, all requests
/// are allowed through (localhost development mode). If set, requests must
/// include a matching `Authorization: Bearer <key>` header.
pub async fn require_auth(request: Request, next: Next) -> Response {
    auth::require_auth(API_KEY_ENV, request, next).await
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
    fn api_key_env_is_correct() {
        assert_eq!(API_KEY_ENV, "DRAVR_CHANNELS_API_KEY");
    }
}
