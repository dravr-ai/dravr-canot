// ABOUTME: Re-exports shared server state from dravr-canot-mcp for unified state management
// ABOUTME: Single SharedState (Arc<RwLock<ServerState>>) used by both REST and MCP endpoints
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

pub use dravr_canot_mcp::state::{ServerState, SharedState};
