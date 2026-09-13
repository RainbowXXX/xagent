use std::sync::Arc;
use crate::config::cli::Cli;
use crate::config::schema::Config;

pub struct App {
    pub(crate) config: Config,
    pub(crate) cli: Cli,
}

impl App {
    pub async fn run(&self) -> anyhow::Result<()> {

        Ok(())
    }

}
