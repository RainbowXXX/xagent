use std::sync::Arc;

use anyhow::{Result, bail};

use crate::config::cli::Cli;
use crate::config::schema::Config;
use crate::frontend::driver::FrontendDriver;
use crate::mcp::{McpConnector, McpManager};
use crate::model::ModelRegistry;
use crate::model::provider::Content;
use crate::runtime::{AgentRuntime, RuntimeExtension, RuntimeHandle, TurnOutcome};
use crate::session::SessionStore;
use crate::tool::ToolRegistry;

pub struct App {
    pub(crate) config: Config,
    pub(crate) cli: Cli,
    pub(crate) models: Arc<ModelRegistry>,
    pub(crate) tools: Arc<ToolRegistry>,
    pub(crate) sessions: Arc<dyn SessionStore>,
    pub(crate) mcp: McpManager,
    pub(crate) runtime: Arc<AgentRuntime>,
    pub(crate) frontend: Option<Box<dyn FrontendDriver>>,
}

impl App {
    pub fn models(&self) -> &Arc<ModelRegistry> {
        &self.models
    }
    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }
    pub fn runtime(&self) -> &Arc<AgentRuntime> {
        &self.runtime
    }

    pub async fn attach_extension(&self, extension: Arc<dyn RuntimeExtension>) -> Result<()> {
        self.runtime.clone().attach_extension(extension).await
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn cli(&self) -> &Cli {
        &self.cli
    }
    pub fn sessions(&self) -> &Arc<dyn SessionStore> {
        &self.sessions
    }

    pub fn set_frontend(&mut self, frontend: Box<dyn FrontendDriver>) {
        self.frontend = Some(frontend);
    }

    pub async fn connect_mcp(&self, connector: &dyn McpConnector) -> Result<()> {
        self.mcp.connect_all(connector, &self.tools).await
    }

    pub async fn submit(&self, session_id: &str, input: Content) -> Result<TurnOutcome> {
        self.runtime.submit(session_id, input).await
    }

    pub async fn resume(
        &self,
        session_id: &str,
        call_id: &str,
        approved: bool,
    ) -> Result<TurnOutcome> {
        self.runtime.resume(session_id, call_id, approved).await
    }

    pub async fn continue_turn(&self, session_id: &str) -> Result<TurnOutcome> {
        self.runtime.continue_turn(session_id).await
    }

    pub async fn abort_turn(&self, session_id: &str) -> Result<()> {
        self.runtime.abort_turn(session_id).await
    }

    pub async fn run(&mut self) -> Result<()> {
        let Some(frontend) = &mut self.frontend else {
            bail!("no frontend driver is configured");
        };
        let runtime: Arc<dyn RuntimeHandle> = self.runtime.clone();
        frontend.run(runtime).await
    }
}
