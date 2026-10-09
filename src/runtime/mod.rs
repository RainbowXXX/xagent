use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

use anyhow::{Result, bail};
use tokio::sync::Mutex as AsyncMutex;

mod agent_loop;
mod extension;

pub use extension::ExtensionContext;

use crate::model::ModelRegistry;
use crate::model::provider::{
    BoxFuture, Content, ModelContext, ModelContextUnit, ModelInput, ModelResponse, ToolCall,
    ToolResult,
};
use crate::permission::PermissionEngine;
use crate::session::{ApprovedCall, Session, SessionStatus, SessionStore};
use crate::tool::ToolRegistry;
use agent_loop::AgentLoop;
use extension::QueuedInput;

#[derive(Clone, Debug)]
pub enum RuntimeEvent {
    TurnStarted {
        session_id: String,
    },
    ModelInvoked {
        session_id: String,
    },
    ModelResponded {
        session_id: String,
        response: ModelResponse,
    },
    ExtensionInputAdded {
        session_id: String,
        source: String,
        content: Content,
    },
    ToolStarted {
        session_id: String,
        call: ToolCall,
    },
    ToolFinished {
        session_id: String,
        result: ToolResult,
    },
    ApprovalRequested {
        session_id: String,
        call: ToolCall,
    },
    ApprovalResolved {
        session_id: String,
        call_id: String,
        approved: bool,
    },
    TurnCompleted {
        session_id: String,
        content: Content,
    },
    TurnFailed {
        session_id: String,
        error: String,
    },
    TurnAborted {
        session_id: String,
    },
}

/// Extensions can transform model context, observe events, and start background work on attach.
pub trait RuntimeExtension: Send + Sync {
    /// Start background work here and return promptly; the work can use ExtensionContext later.
    fn on_attach(&self, _context: ExtensionContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn before_model<'a>(
        &'a self,
        _session: &'a Session,
        _context: &'a mut ModelContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn after_model<'a>(
        &'a self,
        _session: &'a Session,
        _response: &'a mut ModelResponse,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn on_event(&self, _event: RuntimeEvent) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TurnOutcome {
    Completed(Content),
    ApprovalRequired(ToolCall),
}

/// The frontend depends on this interface, not on a particular runtime implementation.
pub trait RuntimeHandle: Send + Sync + 'static {
    fn submit<'a>(
        &'a self,
        session_id: &'a str,
        input: Content,
    ) -> BoxFuture<'a, Result<TurnOutcome>>;
    fn resume<'a>(
        &'a self,
        session_id: &'a str,
        call_id: &'a str,
        approved: bool,
    ) -> BoxFuture<'a, Result<TurnOutcome>>;
    fn continue_turn<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<TurnOutcome>>;
    fn abort_turn<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>>;
    fn session(&self, session_id: &str) -> Result<Option<Session>>;
    fn add_extension(&self, extension: Arc<dyn RuntimeExtension>);
    fn attach_extension(
        self: Arc<Self>,
        extension: Arc<dyn RuntimeExtension>,
    ) -> BoxFuture<'static, Result<()>>;
}

pub struct AgentRuntime {
    model: String,
    models: Arc<ModelRegistry>,
    tools: Arc<ToolRegistry>,
    sessions: Arc<dyn SessionStore>,
    permissions: Arc<dyn PermissionEngine>,
    extensions: RwLock<Vec<Arc<dyn RuntimeExtension>>>,
    extension_inbox: Mutex<HashMap<String, VecDeque<QueuedInput>>>,
    session_locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    max_model_steps: AtomicUsize,
}

impl AgentRuntime {
    pub fn new(
        model: String,
        models: Arc<ModelRegistry>,
        tools: Arc<ToolRegistry>,
        sessions: Arc<dyn SessionStore>,
        permissions: Arc<dyn PermissionEngine>,
    ) -> Result<Self> {
        if model.is_empty() {
            bail!("configured model must not be empty");
        }
        Ok(Self {
            model,
            models,
            tools,
            sessions,
            permissions,
            extensions: RwLock::new(Vec::new()),
            extension_inbox: Mutex::new(HashMap::new()),
            session_locks: Mutex::new(HashMap::new()),
            max_model_steps: AtomicUsize::new(16),
        })
    }

    pub fn add_extension(&self, extension: Arc<dyn RuntimeExtension>) {
        self.extensions
            .write()
            .expect("runtime extension lock poisoned")
            .push(extension);
    }

    /// Attach an extension that may start background work and post back into this runtime.
    pub async fn attach_extension(
        self: &Arc<Self>,
        extension: Arc<dyn RuntimeExtension>,
    ) -> Result<()> {
        let context = ExtensionContext::new(self)?;
        self.add_extension(extension.clone());
        if let Err(error) = extension.on_attach(context).await {
            let mut extensions = self
                .extensions
                .write()
                .expect("runtime extension lock poisoned");
            if let Some(index) = extensions
                .iter()
                .rposition(|item| Arc::ptr_eq(item, &extension))
            {
                extensions.remove(index);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn set_max_model_steps(&self, steps: usize) -> Result<()> {
        if steps == 0 {
            bail!("max_model_steps must be greater than zero");
        }
        self.max_model_steps.store(steps, Ordering::Relaxed);
        Ok(())
    }

    /// Start a turn. The same session cannot accept another turn while approval is pending.
    pub async fn submit(&self, session_id: &str, input: Content) -> Result<TurnOutcome> {
        self.submit_input(session_id, ModelInput::UserInput(input))
            .await
    }

    pub(crate) async fn submit_extension(
        &self,
        session_id: &str,
        source: String,
        content: Content,
    ) -> Result<TurnOutcome> {
        self.submit_input(session_id, ModelInput::ExtensionInput { source, content })
            .await
    }

    async fn submit_input(&self, session_id: &str, input: ModelInput) -> Result<TurnOutcome> {
        if session_id.trim().is_empty() {
            bail!("session id must not be empty");
        }
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().await;
        let mut session = self
            .sessions
            .load(session_id)?
            .unwrap_or_else(|| Session::new(session_id));
        if session.status != SessionStatus::Idle || !session.pending_calls.is_empty() {
            bail!("session has an unfinished turn: {session_id}");
        }
        session.status = SessionStatus::Running;
        session.model_steps = 0;
        session.approved_call = None;
        session.context.units.push(ModelContextUnit::Input(input));
        self.sessions.save(&session)?;
        self.emit(RuntimeEvent::TurnStarted {
            session_id: session_id.into(),
        })
        .await;
        self.drive_with_error_event(&mut session).await
    }

    /// Retry an interrupted turn, including after restart with a persistent SessionStore.
    pub async fn continue_turn(&self, session_id: &str) -> Result<TurnOutcome> {
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().await;
        let mut session = self
            .sessions
            .load(session_id)?
            .ok_or_else(|| anyhow::anyhow!("session does not exist: {session_id}"))?;
        if session.status != SessionStatus::Running {
            bail!("session has no running turn: {session_id}");
        }
        self.drive_with_error_event(&mut session).await
    }

    /// Continue a turn after the caller has shown the pending call and obtained a decision.
    pub async fn resume(
        &self,
        session_id: &str,
        call_id: &str,
        approved: bool,
    ) -> Result<TurnOutcome> {
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().await;
        let mut session = self
            .sessions
            .load(session_id)?
            .ok_or_else(|| anyhow::anyhow!("session does not exist: {session_id}"))?;
        if session.status != SessionStatus::AwaitingApproval {
            bail!("session is not awaiting approval: {session_id}");
        }
        let call = session
            .pending_calls
            .front()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("pending approval has no tool call"))?;
        if call.id != call_id {
            bail!("approval call id does not match pending call");
        }
        session.status = SessionStatus::Running;
        session.approved_call = Some(ApprovedCall {
            call_id: call_id.into(),
            approved,
        });
        self.sessions.save(&session)?;
        self.emit(RuntimeEvent::ApprovalResolved {
            session_id: session.id.clone(),
            call_id: call_id.into(),
            approved,
        })
        .await;
        self.drive_with_error_event(&mut session).await
    }

    /// Discard unfinished work after any active run call releases the session lock.
    pub async fn abort_turn(&self, session_id: &str) -> Result<()> {
        let lock = self.session_lock(session_id);
        let _guard = lock.lock().await;
        let mut session = self
            .sessions
            .load(session_id)?
            .ok_or_else(|| anyhow::anyhow!("session does not exist: {session_id}"))?;
        if session.status == SessionStatus::Idle {
            return Ok(());
        }
        let mut aborted_results = Vec::new();
        for call in session.pending_calls.drain(..) {
            let result = ToolResult {
                call_id: call.id,
                content: Content::Text("Turn aborted before tool execution".into()),
                is_error: true,
            };
            session
                .context
                .units
                .push(ModelContextUnit::Input(ModelInput::ToolResult(
                    result.clone(),
                )));
            aborted_results.push(result);
        }
        session.approved_call = None;
        session.model_steps = 0;
        session.status = SessionStatus::Idle;
        self.sessions.save(&session)?;
        for result in aborted_results {
            self.emit(RuntimeEvent::ToolFinished {
                session_id: session_id.into(),
                result,
            })
            .await;
        }
        self.emit(RuntimeEvent::TurnAborted {
            session_id: session_id.into(),
        })
        .await;
        Ok(())
    }

    async fn drive_with_error_event(&self, session: &mut Session) -> Result<TurnOutcome> {
        let result = AgentLoop::new(self, session).run().await;
        if let Err(error) = &result {
            self.emit(RuntimeEvent::TurnFailed {
                session_id: session.id.clone(),
                error: error.to_string(),
            })
            .await;
        }
        result
    }

    fn session_lock(&self, id: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self
            .session_locks
            .lock()
            .expect("session lock map poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(id.to_owned(), Arc::downgrade(&lock));
        lock
    }

    fn queue_extension_input(&self, session_id: &str, input: QueuedInput) {
        self.extension_inbox
            .lock()
            .expect("extension inbox lock poisoned")
            .entry(session_id.into())
            .or_default()
            .push_back(input);
    }

    fn take_extension_inputs(&self, session_id: &str) -> VecDeque<QueuedInput> {
        self.extension_inbox
            .lock()
            .expect("extension inbox lock poisoned")
            .remove(session_id)
            .unwrap_or_default()
    }

    fn restore_extension_inputs(&self, session_id: &str, mut inputs: VecDeque<QueuedInput>) {
        let mut inbox = self
            .extension_inbox
            .lock()
            .expect("extension inbox lock poisoned");
        let entry = inbox.entry(session_id.into()).or_default();
        inputs.append(entry);
        *entry = inputs;
    }

    fn extensions(&self) -> Vec<Arc<dyn RuntimeExtension>> {
        self.extensions
            .read()
            .expect("runtime extension lock poisoned")
            .clone()
    }

    async fn emit(&self, event: RuntimeEvent) {
        for extension in self.extensions() {
            extension.on_event(event.clone()).await;
        }
    }
}

impl RuntimeHandle for AgentRuntime {
    fn submit<'a>(
        &'a self,
        session_id: &'a str,
        input: Content,
    ) -> BoxFuture<'a, Result<TurnOutcome>> {
        Box::pin(async move { AgentRuntime::submit(self, session_id, input).await })
    }

    fn resume<'a>(
        &'a self,
        session_id: &'a str,
        call_id: &'a str,
        approved: bool,
    ) -> BoxFuture<'a, Result<TurnOutcome>> {
        Box::pin(async move { AgentRuntime::resume(self, session_id, call_id, approved).await })
    }

    fn continue_turn<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<TurnOutcome>> {
        Box::pin(async move { AgentRuntime::continue_turn(self, session_id).await })
    }

    fn abort_turn<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { AgentRuntime::abort_turn(self, session_id).await })
    }

    fn session(&self, session_id: &str) -> Result<Option<Session>> {
        self.sessions.load(session_id)
    }

    fn add_extension(&self, extension: Arc<dyn RuntimeExtension>) {
        AgentRuntime::add_extension(self, extension);
    }

    fn attach_extension(
        self: Arc<Self>,
        extension: Arc<dyn RuntimeExtension>,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move { AgentRuntime::attach_extension(&self, extension).await })
    }
}
