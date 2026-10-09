use std::collections::HashMap;

use anyhow::{Result, bail};

use crate::config::schema::Config;
use crate::model::provider::{BoxFuture, ToolCall};
use crate::session::Session;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

pub trait PermissionEngine: Send + Sync {
    fn decide<'a>(
        &'a self,
        session: &'a Session,
        call: &'a ToolCall,
    ) -> BoxFuture<'a, Result<PermissionDecision>>;
}

pub struct ConfigPermissionEngine {
    server_modes: HashMap<String, PermissionDecision>,
    default_mode: PermissionDecision,
}

impl ConfigPermissionEngine {
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut server_modes = HashMap::new();
        for (server, settings) in &config.mcp_servers {
            let mode = match settings.default_tools_approval_mode.as_str() {
                "" | "ask" => PermissionDecision::Ask,
                "allow" => PermissionDecision::Allow,
                "deny" => PermissionDecision::Deny,
                other => bail!("invalid approval mode for MCP server {server}: {other}"),
            };
            server_modes.insert(server.clone(), mode);
        }
        Ok(Self {
            server_modes,
            default_mode: PermissionDecision::Ask,
        })
    }
}

impl PermissionEngine for ConfigPermissionEngine {
    fn decide<'a>(
        &'a self,
        _session: &'a Session,
        call: &'a ToolCall,
    ) -> BoxFuture<'a, Result<PermissionDecision>> {
        Box::pin(async move {
            let Some((server, _)) = call.name.split_once('/') else {
                return Ok(self.default_mode);
            };
            Ok(self
                .server_modes
                .get(server)
                .copied()
                .unwrap_or(self.default_mode))
        })
    }
}
