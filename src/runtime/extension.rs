use std::future::Future;
use std::sync::{Arc, Weak};

use anyhow::{Context, Result, bail};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::{AgentRuntime, TurnOutcome};
use crate::model::provider::Content;
use crate::session::Session;

#[derive(Clone, Debug)]
pub(super) struct QueuedInput {
    pub source: String,
    pub content: Content,
}

/// A cloneable, nonblocking handle for work that continues after an extension callback returns.
#[derive(Clone)]
pub struct ExtensionContext {
    runtime: Weak<AgentRuntime>,
    executor: Handle,
}

impl ExtensionContext {
    pub(super) fn new(runtime: &Arc<AgentRuntime>) -> Result<Self> {
        Ok(Self {
            runtime: Arc::downgrade(runtime),
            executor: Handle::try_current()
                .context("attaching an extension requires a Tokio runtime")?,
        })
    }

    pub fn session(&self, session_id: &str) -> Result<Option<Session>> {
        self.runtime()?.sessions.load(session_id)
    }

    /// Queue context for the next safe Agent Loop boundary. This never waits for the session lock.
    pub fn post_context(&self, session_id: &str, source: &str, content: Content) -> Result<()> {
        if session_id.trim().is_empty() || source.trim().is_empty() {
            bail!("session id and extension source must not be empty");
        }
        self.runtime()?.queue_extension_input(
            session_id,
            QueuedInput {
                source: source.into(),
                content,
            },
        );
        Ok(())
    }

    /// Schedule a new turn without blocking the extension. Dropping the handle detaches the task.
    pub fn start_turn(
        &self,
        session_id: &str,
        source: &str,
        content: Content,
    ) -> Result<JoinHandle<Result<TurnOutcome>>> {
        if session_id.trim().is_empty() || source.trim().is_empty() {
            bail!("session id and extension source must not be empty");
        }
        let runtime = self.runtime()?;
        let session_id = session_id.to_owned();
        let source = source.to_owned();
        Ok(self
            .executor
            .spawn(async move { runtime.submit_extension(&session_id, source, content).await }))
    }

    /// Run a background watcher or timer without holding up the Agent Loop.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor.spawn(future)
    }

    fn runtime(&self) -> Result<Arc<AgentRuntime>> {
        self.runtime
            .upgrade()
            .context("agent runtime is no longer available")
    }
}
