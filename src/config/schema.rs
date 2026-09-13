use std::path::Path;
use std::collections::HashMap;

use anyhow::Context;
use serde::Deserialize;
use crate::config::cli::Cli;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub(crate) model: String,

    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Deserialize)]
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
    pub(crate) fn from_path(config_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config_path = config_path.as_ref();

        let config_content = std::fs::read_to_string(config_path)
            .with_context(|| {
                format!(
                    "failed to read config file: {}",
                    config_path.display()
                )
            })?;

        let config = toml::from_str(&config_content)
            .with_context(|| {
                format!(
                    "failed to parse config file as TOML: {}",
                    config_path.display()
                )
            })?;

        Ok(config)
    }

    pub(crate) fn with_cli_overrides(mut self, cli: &Cli) -> Self {
        self
    }
}
