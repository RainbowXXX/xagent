use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::model::provider::{ModelContext, ToolCall};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub id: String,
    pub context: ModelContext,
    pub pending_calls: VecDeque<ToolCall>,
    pub status: SessionStatus,
    #[serde(default)]
    pub(crate) model_steps: usize,
    #[serde(default)]
    pub(crate) approved_call: Option<ApprovedCall>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ApprovedCall {
    pub call_id: String,
    pub approved: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum SessionStatus {
    Idle,
    Running,
    AwaitingApproval,
}

impl Session {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            context: ModelContext::default(),
            pending_calls: VecDeque::new(),
            status: SessionStatus::Idle,
            model_steps: 0,
            approved_call: None,
        }
    }
}

/// A persistent implementation can replace the in-memory store without changing the runtime.
pub trait SessionStore: Send + Sync {
    fn load(&self, id: &str) -> Result<Option<Session>>;
    fn save(&self, session: &Session) -> Result<()>;
}

#[derive(Default)]
pub struct InMemorySessionStore {
    sessions: RwLock<HashMap<String, Session>>,
}

impl SessionStore for InMemorySessionStore {
    fn load(&self, id: &str) -> Result<Option<Session>> {
        Ok(self
            .sessions
            .read()
            .expect("session store lock poisoned")
            .get(id)
            .cloned())
    }

    fn save(&self, session: &Session) -> Result<()> {
        self.sessions
            .write()
            .expect("session store lock poisoned")
            .insert(session.id.clone(), session.clone());
        Ok(())
    }
}
