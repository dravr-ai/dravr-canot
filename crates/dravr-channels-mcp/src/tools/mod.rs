// ABOUTME: Tool registry for MCP tool definitions and dispatch
// ABOUTME: Maps tool names to async handler functions for channels management
//
// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 dravr.ai

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::protocol::{CallToolResult, ToolDefinition};
use crate::state::SharedState;

/// Type alias for an async tool handler function
pub type ToolHandler = Arc<
    dyn Fn(SharedState, Value) -> Pin<Box<dyn Future<Output = CallToolResult> + Send>>
        + Send
        + Sync,
>;

/// Registry mapping tool names to their definitions and handlers
pub struct ToolRegistry {
    tools: HashMap<String, (ToolDefinition, ToolHandler)>,
}

impl ToolRegistry {
    /// Create an empty tool registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool with its definition and handler
    pub fn register(&mut self, definition: ToolDefinition, handler: ToolHandler) {
        self.tools
            .insert(definition.name.clone(), (definition, handler));
    }

    /// List all registered tool definitions
    pub fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|(def, _)| def.clone()).collect()
    }

    /// Execute a tool by name with the given arguments
    pub async fn execute(
        &self,
        name: &str,
        state: &SharedState,
        arguments: Value,
    ) -> CallToolResult {
        match self.tools.get(name) {
            Some((_, handler)) => handler(Arc::clone(state), arguments).await,
            None => CallToolResult::error(format!("Unknown tool: {name}")),
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default tool registry with messaging channel tools
pub fn build_tool_registry() -> ToolRegistry {
    ToolRegistry::new()
}
