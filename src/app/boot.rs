use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use crate::config::cli::Cli;
use crate::config::schema::Config;

use super::app::App;

pub async fn boot(cli: Cli, config_path: PathBuf) -> Result<App> {
    let config = Config::from_path(&config_path)?
        .with_cli_overrides(&cli);

    // init CLI UI

    // init session store
    // init model registry
    // init tool registry
    // init MCP manager
    // init permission engine
    // init context builder
    // init agent runtime

    Ok(App {
        config,
        cli,
    })
}