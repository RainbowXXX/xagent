use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::model::provider::{BoxFuture, Content, ToolDefinition};

pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn call(&self, arguments: Value) -> BoxFuture<'_, Result<Content>>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn register(&self, tool: Arc<dyn Tool>) -> Result<()> {
        self.register_many(vec![tool])
    }

    /// Validate a batch before publishing any of its tools.
    pub fn register_many(&self, batch: Vec<Arc<dyn Tool>>) -> Result<()> {
        let mut incoming = HashMap::new();
        for tool in batch {
            let name = tool.definition().name;
            if name.is_empty() {
                bail!("tool name must not be empty");
            }
            if incoming.insert(name.clone(), tool).is_some() {
                bail!("tool is repeated in registration batch: {name}");
            }
        }
        let mut tools = self.tools.write().expect("tool registry lock poisoned");
        for name in incoming.keys() {
            if tools.contains_key(name) {
                bail!("tool already registered: {name}");
            }
        }
        tools.extend(incoming);
        Ok(())
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let tools = self.tools.read().expect("tool registry lock poisoned");
        let mut definitions: Vec<_> = tools.values().map(|tool| tool.definition()).collect();
        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        definitions
    }

    pub async fn call(&self, name: &str, arguments: Value) -> Result<Content> {
        let tool = self
            .tools
            .read()
            .expect("tool registry lock poisoned")
            .get(name)
            .cloned()
            .with_context(|| format!("tool is not registered: {name}"))?;
        tool.call(arguments).await
    }
}
