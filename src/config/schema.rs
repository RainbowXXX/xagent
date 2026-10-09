use std::collections::HashMap;
use std::path::Path;

use crate::config::cli::Cli;
use anyhow::Context;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub model: String,

    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct McpServerConfig {
    #[serde(default)]
    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default)]
    pub default_tools_approval_mode: String,
}

impl Config {
    pub fn from_path(config_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config_path = config_path.as_ref();

        let config_content = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path.display()))?;

        let config = toml::from_str(&config_content).with_context(|| {
            format!(
                "failed to parse config file as TOML: {}",
                config_path.display()
            )
        })?;

        Ok(config)
    }

    pub fn with_cli_overrides(mut self, cli: &Cli) -> Self {
        if let Some(model) = &cli.model {
            self.model = model.clone();
        }
        self
    }
}
