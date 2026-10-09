use anyhow::Context;
use std::path::PathBuf;
use xagent::app::boot::boot;
use xagent::config::cli::{Cli, DEFAULT_CONFIG_DIR, DEFAULT_CONFIG_FILE};

fn path_from_cli(cli: &Cli) -> anyhow::Result<PathBuf> {
    if let Some(path) = cli.config.clone() {
        return Ok(path);
    }

    let config_dir = dirs::config_dir().context("Could not find user config directory")?;

    Ok(config_dir
        .join(DEFAULT_CONFIG_DIR)
        .join(DEFAULT_CONFIG_FILE))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::load();
    let config_path = path_from_cli(&cli)?;

    let mut app = boot(cli, config_path).await?;
    app.run().await?;

    Ok(())
}
