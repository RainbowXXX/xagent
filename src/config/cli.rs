use std::path::PathBuf;
use clap::{Subcommand, Parser};

pub(crate) const DEFAULT_CONFIG_DIR: &'static str = ".xagent";
pub(crate) const DEFAULT_CONFIG_FILE: &'static str = "config.toml";

#[derive(Parser)]
pub(crate) struct Cli {
    #[arg(short, long)]
    pub(crate) verbose: bool,
    #[arg(short, long)]
    pub(crate) config: Option<PathBuf>,

    #[clap(subcommand)]
    pub(crate) sub_command: SubCommand,
}

#[derive(Subcommand)]
pub(crate) enum SubCommand {

}

impl Cli {
    pub(crate) fn load() -> Self {
        Self::parse()
    }
}