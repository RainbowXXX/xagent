use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::schema::McpServerConfig;
use crate::model::provider::{BoxFuture, Content, ToolDefinition};
use crate::tool::{Tool, ToolRegistry};
use serde_json::Value;

/// The transport and protocol implementation live behind this boundary.
pub trait McpConnector: Send + Sync {
    fn connect<'a>(
        &'a self,
        name: &'a str,
        config: &'a McpServerConfig,
    ) -> BoxFuture<'a, Result<Vec<Arc<dyn Tool>>>>;
}

struct NamespacedTool {
    name: String,
    inner: Arc<dyn Tool>,
}

impl Tool for NamespacedTool {
    fn definition(&self) -> ToolDefinition {
        let mut definition = self.inner.definition();
        definition.name = self.name.clone();
        definition
    }

    fn call(&self, arguments: Value) -> BoxFuture<'_, Result<Content>> {
        self.inner.call(arguments)
    }
}

pub struct McpManager {
    servers: HashMap<String, McpServerConfig>,
}

impl McpManager {
    pub fn new(servers: HashMap<String, McpServerConfig>) -> Self {
        Self { servers }
    }

    pub async fn connect_all(
        &self,
        connector: &dyn McpConnector,
        tools: &ToolRegistry,
    ) -> Result<()> {
        let mut names: Vec<_> = self.servers.keys().collect();
        names.sort();
        let mut discovered: Vec<Arc<dyn Tool>> = Vec::new();
        for name in names {
            let config = &self.servers[name];
            let connected = connector
                .connect(name, config)
                .await
                .with_context(|| format!("failed to connect MCP server: {name}"))?;
            for tool in connected {
                let tool_name = tool.definition().name;
                let namespaced = NamespacedTool {
                    name: format!("{name}/{tool_name}"),
                    inner: tool,
                };
                discovered.push(Arc::new(namespaced));
            }
        }
        tools
            .register_many(discovered)
            .context("failed to register MCP tools")
    }
}
