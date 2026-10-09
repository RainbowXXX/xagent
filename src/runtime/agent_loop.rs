use std::collections::HashSet;
use std::sync::atomic::Ordering;

use anyhow::{Result, bail};

use super::extension::QueuedInput;
use super::{AgentRuntime, RuntimeEvent, TurnOutcome};
use crate::model::provider::{
    Content, ModelContextUnit, ModelInput, ModelOutput, ModelResponse, ToolCall, ToolResult,
};
use crate::permission::PermissionDecision;
use crate::session::{Session, SessionStatus};

/// Drives one persisted turn until it completes, pauses for approval, or fails.
pub(super) struct AgentLoop<'a> {
    runtime: &'a AgentRuntime,
    session: &'a mut Session,
}

impl<'a> AgentLoop<'a> {
    pub(super) fn new(runtime: &'a AgentRuntime, session: &'a mut Session) -> Self {
        Self { runtime, session }
    }

    pub(super) async fn run(&mut self) -> Result<TurnOutcome> {
        loop {
            let _ = self.drain_extension_inputs().await?;
            if let Some(call) = self.session.pending_calls.front().cloned() {
                let decision = match &self.session.approved_call {
                    Some(approval) if approval.call_id == call.id => {
                        if approval.approved {
                            PermissionDecision::Allow
                        } else {
                            PermissionDecision::Deny
                        }
                    }
                    Some(_) => bail!("saved approval does not match the pending tool call"),
                    None => self.runtime.permissions.decide(self.session, &call).await?,
                };
                match decision {
                    PermissionDecision::Allow => self.finish_call(call, true).await?,
                    PermissionDecision::Deny => self.finish_call(call, false).await?,
                    PermissionDecision::Ask => return self.pause_for_approval(call).await,
                }
                continue;
            }

            if self.session.approved_call.is_some() {
                bail!("saved approval has no pending tool call");
            }
            let limit = self.runtime.max_model_steps.load(Ordering::Relaxed);
            if self.session.model_steps >= limit {
                bail!(
                    "model step limit reached for session {} ({limit} steps)",
                    self.session.id
                );
            }
            if let Some(outcome) = self.invoke_model().await? {
                return Ok(outcome);
            }
        }
    }

    async fn drain_extension_inputs(&mut self) -> Result<bool> {
        let queued = self.runtime.take_extension_inputs(&self.session.id);
        if queued.is_empty() {
            return Ok(false);
        }
        self.append_extension_inputs(&queued);
        if let Err(error) = self.runtime.sessions.save(self.session) {
            self.runtime
                .restore_extension_inputs(&self.session.id, queued);
            return Err(error);
        }
        self.emit_extension_inputs(queued).await;
        Ok(true)
    }

    fn append_extension_inputs(&mut self, queued: &std::collections::VecDeque<QueuedInput>) {
        for input in queued {
            self.session
                .context
                .units
                .push(ModelContextUnit::Input(ModelInput::ExtensionInput {
                    source: input.source.clone(),
                    content: input.content.clone(),
                }));
        }
    }

    async fn emit_extension_inputs(&self, queued: std::collections::VecDeque<QueuedInput>) {
        for input in queued {
            self.runtime
                .emit(RuntimeEvent::ExtensionInputAdded {
                    session_id: self.session.id.clone(),
                    source: input.source,
                    content: input.content,
                })
                .await;
        }
    }

    async fn pause_for_approval(&mut self, call: ToolCall) -> Result<TurnOutcome> {
        self.session.status = SessionStatus::AwaitingApproval;
        self.runtime.sessions.save(self.session)?;
        self.runtime
            .emit(RuntimeEvent::ApprovalRequested {
                session_id: self.session.id.clone(),
                call: call.clone(),
            })
            .await;
        Ok(TurnOutcome::ApprovalRequired(call))
    }

    async fn invoke_model(&mut self) -> Result<Option<TurnOutcome>> {
        let extensions = self.runtime.extensions();
        let mut context = self.session.context.clone();
        for extension in &extensions {
            extension.before_model(self.session, &mut context).await?;
        }
        self.runtime
            .emit(RuntimeEvent::ModelInvoked {
                session_id: self.session.id.clone(),
            })
            .await;
        let mut response = self
            .runtime
            .models
            .invoke(
                &self.runtime.model,
                &context,
                &self.runtime.tools.definitions(),
            )
            .await?;
        for extension in &extensions {
            extension.after_model(self.session, &mut response).await?;
        }
        let final_content = validate_response(self.session, &response)?;
        self.session.model_steps += 1;
        for output in &response.outputs {
            if let ModelOutput::ToolCall(call) = output {
                self.session.pending_calls.push_back(call.clone());
            }
            self.session
                .context
                .units
                .push(ModelContextUnit::Output(output.clone()));
        }
        let queued = self.runtime.take_extension_inputs(&self.session.id);
        self.append_extension_inputs(&queued);
        let had_queued_input = !queued.is_empty();
        if let Err(error) = self.runtime.sessions.save(self.session) {
            self.runtime
                .restore_extension_inputs(&self.session.id, queued);
            return Err(error);
        }
        self.runtime
            .emit(RuntimeEvent::ModelResponded {
                session_id: self.session.id.clone(),
                response,
            })
            .await;
        self.emit_extension_inputs(queued).await;
        if let Some(content) = final_content {
            if self.session.pending_calls.is_empty()
                && !had_queued_input
                && !self.drain_extension_inputs().await?
            {
                self.session.status = SessionStatus::Idle;
                self.session.model_steps = 0;
                self.runtime.sessions.save(self.session)?;
                self.runtime
                    .emit(RuntimeEvent::TurnCompleted {
                        session_id: self.session.id.clone(),
                        content: content.clone(),
                    })
                    .await;
                return Ok(Some(TurnOutcome::Completed(content)));
            }
        }
        Ok(None)
    }

    async fn finish_call(&mut self, call: ToolCall, approved: bool) -> Result<()> {
        let result = if approved {
            self.runtime
                .emit(RuntimeEvent::ToolStarted {
                    session_id: self.session.id.clone(),
                    call: call.clone(),
                })
                .await;
            match self
                .runtime
                .tools
                .call(&call.name, call.arguments.clone())
                .await
            {
                Ok(content) => ToolResult {
                    call_id: call.id.clone(),
                    content,
                    is_error: false,
                },
                Err(error) => ToolResult {
                    call_id: call.id.clone(),
                    content: Content::Text(error.to_string()),
                    is_error: true,
                },
            }
        } else {
            ToolResult {
                call_id: call.id.clone(),
                content: Content::Text("Tool call denied by permission policy".into()),
                is_error: true,
            }
        };
        self.session.pending_calls.pop_front();
        self.session.approved_call = None;
        self.session
            .context
            .units
            .push(ModelContextUnit::Input(ModelInput::ToolResult(
                result.clone(),
            )));
        self.runtime.sessions.save(self.session)?;
        self.runtime
            .emit(RuntimeEvent::ToolFinished {
                session_id: self.session.id.clone(),
                result,
            })
            .await;
        Ok(())
    }
}

fn validate_response(session: &Session, response: &ModelResponse) -> Result<Option<Content>> {
    if response.outputs.is_empty() {
        bail!("model returned no output");
    }
    let mut call_ids: HashSet<&str> = session
        .context
        .units
        .iter()
        .filter_map(|unit| match unit {
            ModelContextUnit::Output(ModelOutput::ToolCall(call)) => Some(call.id.as_str()),
            _ => None,
        })
        .collect();
    let mut has_tool_call = false;
    let mut final_content = None;
    for output in &response.outputs {
        match output {
            ModelOutput::Chat(content) => final_content = Some(content.clone()),
            ModelOutput::ToolCall(call) => {
                if call.id.trim().is_empty() || call.name.trim().is_empty() {
                    bail!("model returned a tool call without an id or name");
                }
                if !call_ids.insert(&call.id) {
                    bail!("model returned a duplicate tool call id: {}", call.id);
                }
                has_tool_call = true;
            }
            ModelOutput::Reasoning(_) => {}
        }
    }
    if !has_tool_call && final_content.is_none() {
        bail!("model returned neither a final message nor a tool call");
    }
    Ok(final_content)
}
