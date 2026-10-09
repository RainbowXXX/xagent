use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::config::cli::Cli;
use crate::config::schema::Config;
use crate::mcp::McpManager;
use crate::model::ModelRegistry;
use crate::permission::{ConfigPermissionEngine, PermissionEngine};
use crate::runtime::AgentRuntime;
use crate::session::{InMemorySessionStore, SessionStore};
use crate::tool::ToolRegistry;

use super::app::App;

#[derive(Default)]
pub struct BootOptions {
    pub sessions: Option<Arc<dyn SessionStore>>,
    pub permissions: Option<Arc<dyn PermissionEngine>>,
}

pub async fn boot(cli: Cli, config_path: PathBuf) -> Result<App> {
    boot_with(cli, config_path, BootOptions::default()).await
}

pub async fn boot_with(cli: Cli, config_path: PathBuf, options: BootOptions) -> Result<App> {
    let config = Config::from_path(&config_path)?.with_cli_overrides(&cli);

    let models = Arc::new(ModelRegistry::default());
    let tools = Arc::new(ToolRegistry::default());
    let sessions: Arc<dyn SessionStore> = options
        .sessions
        .unwrap_or_else(|| Arc::new(InMemorySessionStore::default()));
    let permissions: Arc<dyn PermissionEngine> = match options.permissions {
        Some(permissions) => permissions,
        None => Arc::new(ConfigPermissionEngine::from_config(&config)?),
    };
    let mcp = McpManager::new(config.mcp_servers.clone());
    let runtime = Arc::new(AgentRuntime::new(
        config.model.clone(),
        models.clone(),
        tools.clone(),
        sessions.clone(),
        permissions,
    )?);

    Ok(App {
        config,
        cli,
        models,
        tools,
        sessions,
        mcp,
        runtime,
        frontend: None,
    })
}
