use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub const DEFAULT_CONFIG_DIR: &str = ".xagent";
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";

#[derive(Parser)]
pub struct Cli {
    #[arg(short, long)]
    pub verbose: bool,
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[arg(long)]
    pub model: Option<String>,

    #[clap(subcommand)]
    pub sub_command: Option<SubCommand>,
}

#[derive(Subcommand)]
pub enum SubCommand {}

impl Cli {
    pub fn load() -> Self {
        Self::parse()
    }
}
