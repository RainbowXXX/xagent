use std::sync::Arc;

use crate::model::provider::BoxFuture;
use crate::runtime::RuntimeHandle;

/// A CLI, TUI, or other host can drive the same agent runtime.
pub trait FrontendDriver: Send {
    fn run(&mut self, runtime: Arc<dyn RuntimeHandle>) -> BoxFuture<'_, anyhow::Result<()>>;
}
