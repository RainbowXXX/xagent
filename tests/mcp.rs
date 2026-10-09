use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};

use xagent::config::schema::{Config, McpServerConfig};
use xagent::mcp::{McpConnector, McpManager};
use xagent::model::provider::{BoxFuture, Content, ToolCall, ToolDefinition};
use xagent::permission::{ConfigPermissionEngine, PermissionDecision, PermissionEngine};
use xagent::session::Session;
use xagent::tool::{Tool, ToolRegistry};

struct EchoTool;

impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "Echo input".into(),
            parameters: json!({"type": "object"}),
        }
    }

    fn call(&self, _arguments: Value) -> BoxFuture<'_, Result<Content>> {
        Box::pin(async { Ok(Content::Text("ok".into())) })
    }
}

struct FakeConnector;

impl McpConnector for FakeConnector {
    fn connect<'a>(
        &'a self,
        _name: &'a str,
        _config: &'a McpServerConfig,
    ) -> BoxFuture<'a, Result<Vec<Arc<dyn Tool>>>> {
        Box::pin(async { Ok(vec![Arc::new(EchoTool) as Arc<dyn Tool>]) })
    }
}

#[tokio::test]
async fn mcp_tools_use_server_namespace_for_permissions() {
    let server = McpServerConfig {
        command: "fake".into(),
        args: Vec::new(),
        env: HashMap::new(),
        default_tools_approval_mode: "allow".into(),
    };
    let servers = HashMap::from([("docs".into(), server)]);
    let config = Config {
        model: "fake/test".into(),
        mcp_servers: servers.clone(),
    };
    let manager = McpManager::new(servers);
    let tools = ToolRegistry::default();
    manager.connect_all(&FakeConnector, &tools).await.unwrap();

    assert_eq!(tools.definitions()[0].name, "docs/echo");
    assert_eq!(
        tools.call("docs/echo", json!({})).await.unwrap(),
        Content::Text("ok".into()),
    );
    let policy = ConfigPermissionEngine::from_config(&config).unwrap();
    let call = ToolCall {
        id: "1".into(),
        name: "docs/echo".into(),
        arguments: json!({}),
    };
    assert_eq!(
        policy.decide(&Session::new("s1"), &call).await.unwrap(),
        PermissionDecision::Allow
    );
}
