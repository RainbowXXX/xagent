use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde_json::{Value, json};

use xagent::model::ModelRegistry;
use xagent::model::provider::{
    BoxFuture, Content, ModelContext, ModelContextUnit, ModelInput, ModelOutput, ModelProvider,
    ModelResponse, ToolCall, ToolDefinition,
};
use xagent::permission::{PermissionDecision, PermissionEngine};
use xagent::runtime::{AgentRuntime, RuntimeEvent, RuntimeExtension, TurnOutcome};
use xagent::session::{InMemorySessionStore, Session, SessionStatus, SessionStore};
use xagent::tool::{Tool, ToolRegistry};

struct ScriptedProvider {
    responses: Mutex<VecDeque<Result<ModelResponse>>>,
    contexts: Mutex<Vec<ModelContext>>,
}

impl ScriptedProvider {
    fn new(responses: Vec<Result<ModelResponse>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            contexts: Mutex::new(Vec::new()),
        }
    }
}

impl ModelProvider for ScriptedProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn model_list(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(vec!["test".into()]) })
    }

    fn invoke<'a>(
        &'a self,
        _model: &'a str,
        context: &'a ModelContext,
        _tools: &'a [ToolDefinition],
    ) -> BoxFuture<'a, Result<ModelResponse>> {
        self.contexts.lock().unwrap().push(context.clone());
        let response = self.responses.lock().unwrap().pop_front().unwrap();
        Box::pin(async move { response })
    }
}

struct CountingTool(AtomicUsize);

impl Tool for CountingTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "Echo text".into(),
            parameters: json!({"type": "object"}),
        }
    }

    fn call(&self, arguments: Value) -> BoxFuture<'_, Result<Content>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(Content::Text(arguments["text"].as_str().unwrap().into())) })
    }
}

struct FixedPermission(PermissionDecision);

impl PermissionEngine for FixedPermission {
    fn decide<'a>(
        &'a self,
        _session: &'a Session,
        _call: &'a ToolCall,
    ) -> BoxFuture<'a, Result<PermissionDecision>> {
        Box::pin(async move { Ok(self.0) })
    }
}

#[derive(Default)]
struct JsonSessionStore(Mutex<HashMap<String, String>>);

impl SessionStore for JsonSessionStore {
    fn load(&self, id: &str) -> Result<Option<Session>> {
        let saved = self.0.lock().unwrap().get(id).cloned();
        saved
            .map(|json| Ok(serde_json::from_str(&json)?))
            .transpose()
    }

    fn save(&self, session: &Session) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(session.id.clone(), serde_json::to_string(session)?);
        Ok(())
    }
}

#[derive(Default)]
struct StopAfterApprovalStore {
    inner: JsonSessionStore,
    failed_once: std::sync::atomic::AtomicBool,
}

impl SessionStore for StopAfterApprovalStore {
    fn load(&self, id: &str) -> Result<Option<Session>> {
        self.inner.load(id)
    }

    fn save(&self, session: &Session) -> Result<()> {
        self.inner.save(session)?;
        let saved = serde_json::to_value(session)?;
        if saved["approved_call"].is_object() && !self.failed_once.swap(true, Ordering::SeqCst) {
            anyhow::bail!("simulated lost save acknowledgement");
        }
        Ok(())
    }
}

#[derive(Default)]
struct TestExtension {
    events: Mutex<Vec<RuntimeEvent>>,
}

impl RuntimeExtension for TestExtension {
    fn before_model<'a>(
        &'a self,
        _session: &'a Session,
        context: &'a mut ModelContext,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            context
                .units
                .push(ModelContextUnit::Input(ModelInput::UserInput(
                    Content::Text("extension context".into()),
                )));
            Ok(())
        })
    }

    fn on_event(&self, event: RuntimeEvent) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.events.lock().unwrap().push(event) })
    }
}

fn runtime(
    provider: Arc<ScriptedProvider>,
    permission: PermissionDecision,
    sessions: Arc<InMemorySessionStore>,
    tools: Arc<ToolRegistry>,
) -> AgentRuntime {
    let models = Arc::new(ModelRegistry::default());
    models.register(provider).unwrap();
    AgentRuntime::new(
        "fake/test".into(),
        models,
        tools,
        sessions,
        Arc::new(FixedPermission(permission)),
    )
    .unwrap()
}

#[tokio::test]
async fn approval_pauses_and_resumes_tool_turn() {
    let call = ToolCall {
        id: "call-1".into(),
        name: "echo".into(),
        arguments: json!({"text": "hello"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call.clone())],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("done".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let runtime = runtime(
        provider.clone(),
        PermissionDecision::Ask,
        sessions.clone(),
        tools,
    );
    let extension = Arc::new(TestExtension::default());
    runtime.add_extension(extension.clone());

    assert_eq!(
        runtime
            .submit("s1", Content::Text("start".into()))
            .await
            .unwrap(),
        TurnOutcome::ApprovalRequired(call.clone()),
    );
    assert_eq!(
        sessions.load("s1").unwrap().unwrap().status,
        SessionStatus::AwaitingApproval
    );
    assert!(
        runtime
            .submit("s1", Content::Text("again".into()))
            .await
            .is_err()
    );
    assert!(runtime.resume("s1", "wrong-id", true).await.is_err());
    assert_eq!(tool.0.load(Ordering::SeqCst), 0);

    assert_eq!(
        runtime.resume("s1", "call-1", true).await.unwrap(),
        TurnOutcome::Completed(Content::Text("done".into())),
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        sessions.load("s1").unwrap().unwrap().status,
        SessionStatus::Idle
    );
    let contexts = provider.contexts.lock().unwrap();
    assert!(contexts[1].units.iter().any(|unit| matches!(
        unit,
        ModelContextUnit::Input(ModelInput::ToolResult(result)) if result.call_id == "call-1" && !result.is_error
    )));
    assert!(contexts[0].units.iter().any(|unit| matches!(
        unit,
        ModelContextUnit::Input(ModelInput::UserInput(Content::Text(text))) if text == "extension context"
    )));
    assert!(
        extension
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ApprovalRequested { .. }))
    );
}

#[tokio::test]
async fn provider_failure_can_continue_the_same_turn() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        Err(anyhow::anyhow!("temporary provider failure")),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("recovered".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let runtime = runtime(
        provider.clone(),
        PermissionDecision::Deny,
        sessions.clone(),
        Arc::new(ToolRegistry::default()),
    );

    assert!(
        runtime
            .submit("s2", Content::Text("hello".into()))
            .await
            .is_err()
    );
    assert_eq!(
        sessions.load("s2").unwrap().unwrap().status,
        SessionStatus::Running
    );
    assert_eq!(
        runtime.continue_turn("s2").await.unwrap(),
        TurnOutcome::Completed(Content::Text("recovered".into())),
    );
    assert_eq!(
        sessions.load("s2").unwrap().unwrap().status,
        SessionStatus::Idle
    );
    assert_eq!(provider.contexts.lock().unwrap()[1].units.len(), 1);
}

#[tokio::test]
async fn multiple_tool_calls_pause_individually_and_preserve_results() {
    let first = ToolCall {
        id: "first".into(),
        name: "echo".into(),
        arguments: json!({"text": "one"}),
    };
    let second = ToolCall {
        id: "second".into(),
        name: "echo".into(),
        arguments: json!({"text": "two"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![
                ModelOutput::ToolCall(first.clone()),
                ModelOutput::ToolCall(second.clone()),
            ],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("finished".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let runtime = runtime(
        provider.clone(),
        PermissionDecision::Ask,
        sessions.clone(),
        tools,
    );

    assert_eq!(
        runtime
            .submit("multi", Content::Text("start".into()))
            .await
            .unwrap(),
        TurnOutcome::ApprovalRequired(first)
    );
    assert_eq!(
        runtime.resume("multi", "first", true).await.unwrap(),
        TurnOutcome::ApprovalRequired(second)
    );
    assert_eq!(
        runtime.resume("multi", "second", false).await.unwrap(),
        TurnOutcome::Completed(Content::Text("finished".into()))
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
    let contexts = provider.contexts.lock().unwrap();
    let results: Vec<_> = contexts[1]
        .units
        .iter()
        .filter_map(|unit| match unit {
            ModelContextUnit::Input(ModelInput::ToolResult(result)) => Some(result),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].call_id, "first");
    assert!(!results[0].is_error);
    assert_eq!(results[1].call_id, "second");
    assert!(results[1].is_error);
    assert_eq!(
        sessions.load("multi").unwrap().unwrap().status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn model_step_limit_survives_tool_execution_and_continuation() {
    let call = ToolCall {
        id: "one".into(),
        name: "echo".into(),
        arguments: json!({"text": "one"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call)],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("done".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let runtime = runtime(
        provider.clone(),
        PermissionDecision::Allow,
        sessions.clone(),
        tools,
    );
    runtime.set_max_model_steps(1).unwrap();

    assert!(
        runtime
            .submit("limit", Content::Text("start".into()))
            .await
            .is_err()
    );
    assert_eq!(provider.contexts.lock().unwrap().len(), 1);
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
    assert!(runtime.continue_turn("limit").await.is_err());
    assert_eq!(provider.contexts.lock().unwrap().len(), 1);
    assert_eq!(
        sessions.load("limit").unwrap().unwrap().status,
        SessionStatus::Running
    );

    runtime.set_max_model_steps(2).unwrap();
    assert_eq!(
        runtime.continue_turn("limit").await.unwrap(),
        TurnOutcome::Completed(Content::Text("done".into()))
    );
    assert_eq!(provider.contexts.lock().unwrap().len(), 2);
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn invalid_tool_response_is_not_committed() {
    let call = ToolCall {
        id: "duplicate".into(),
        name: "echo".into(),
        arguments: json!({"text": "x"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![
                ModelOutput::ToolCall(call.clone()),
                ModelOutput::ToolCall(call),
            ],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("valid".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let runtime = runtime(
        provider,
        PermissionDecision::Allow,
        sessions.clone(),
        Arc::new(ToolRegistry::default()),
    );

    assert!(
        runtime
            .submit("invalid", Content::Text("start".into()))
            .await
            .is_err()
    );
    let session = sessions.load("invalid").unwrap().unwrap();
    assert_eq!(session.context.units.len(), 1);
    assert!(session.pending_calls.is_empty());
    assert_eq!(
        runtime.continue_turn("invalid").await.unwrap(),
        TurnOutcome::Completed(Content::Text("valid".into()))
    );
}

#[tokio::test]
async fn abort_resolves_pending_calls_before_next_turn() {
    let call = ToolCall {
        id: "pending".into(),
        name: "echo".into(),
        arguments: json!({"text": "x"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call.clone())],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("next".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let runtime = runtime(
        provider,
        PermissionDecision::Ask,
        sessions.clone(),
        Arc::new(ToolRegistry::default()),
    );

    assert_eq!(
        runtime
            .submit("abort", Content::Text("first".into()))
            .await
            .unwrap(),
        TurnOutcome::ApprovalRequired(call)
    );
    runtime.abort_turn("abort").await.unwrap();
    let session = sessions.load("abort").unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Idle);
    assert!(session.pending_calls.is_empty());
    assert!(session.context.units.iter().any(|unit| matches!(unit,
        ModelContextUnit::Input(ModelInput::ToolResult(result)) if result.call_id == "pending" && result.is_error
    )));
    assert_eq!(
        runtime
            .submit("abort", Content::Text("second".into()))
            .await
            .unwrap(),
        TurnOutcome::Completed(Content::Text("next".into()))
    );
}

#[tokio::test]
async fn continuing_after_model_failure_does_not_repeat_completed_tool() {
    let call = ToolCall {
        id: "once".into(),
        name: "echo".into(),
        arguments: json!({"text": "once"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call.clone())],
        }),
        Err(anyhow::anyhow!("provider failed after tool execution")),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("done".into()))],
        }),
    ]));
    let sessions = Arc::new(InMemorySessionStore::default());
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let runtime = runtime(provider, PermissionDecision::Ask, sessions.clone(), tools);

    assert_eq!(
        runtime
            .submit("retry", Content::Text("start".into()))
            .await
            .unwrap(),
        TurnOutcome::ApprovalRequired(call)
    );
    assert!(runtime.resume("retry", "once", true).await.is_err());
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        sessions.load("retry").unwrap().unwrap().status,
        SessionStatus::Running
    );
    assert_eq!(
        runtime.continue_turn("retry").await.unwrap(),
        TurnOutcome::Completed(Content::Text("done".into()))
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn serialized_session_preserves_loop_progress_across_runtime_instances() {
    let call = ToolCall {
        id: "stored".into(),
        name: "echo".into(),
        arguments: json!({"text": "stored"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call)],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("done".into()))],
        }),
    ]));
    let models = Arc::new(ModelRegistry::default());
    models.register(provider.clone()).unwrap();
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let store = Arc::new(JsonSessionStore::default());
    let first_runtime = AgentRuntime::new(
        "fake/test".into(),
        models.clone(),
        tools.clone(),
        store.clone(),
        Arc::new(FixedPermission(PermissionDecision::Allow)),
    )
    .unwrap();
    first_runtime.set_max_model_steps(1).unwrap();
    assert!(
        first_runtime
            .submit("stored", Content::Text("start".into()))
            .await
            .is_err()
    );
    drop(first_runtime);

    let second_runtime = AgentRuntime::new(
        "fake/test".into(),
        models,
        tools,
        store.clone(),
        Arc::new(FixedPermission(PermissionDecision::Allow)),
    )
    .unwrap();
    second_runtime.set_max_model_steps(1).unwrap();
    assert!(second_runtime.continue_turn("stored").await.is_err());
    assert_eq!(provider.contexts.lock().unwrap().len(), 1);
    second_runtime.set_max_model_steps(2).unwrap();
    assert_eq!(
        second_runtime.continue_turn("stored").await.unwrap(),
        TurnOutcome::Completed(Content::Text("done".into()))
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.load("stored").unwrap().unwrap().status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn persisted_approval_is_used_after_restart() {
    let call = ToolCall {
        id: "approved".into(),
        name: "echo".into(),
        arguments: json!({"text": "approved"}),
    };
    let provider = Arc::new(ScriptedProvider::new(vec![
        Ok(ModelResponse {
            outputs: vec![ModelOutput::ToolCall(call.clone())],
        }),
        Ok(ModelResponse {
            outputs: vec![ModelOutput::Chat(Content::Text("done".into()))],
        }),
    ]));
    let models = Arc::new(ModelRegistry::default());
    models.register(provider).unwrap();
    let tools = Arc::new(ToolRegistry::default());
    let tool = Arc::new(CountingTool(AtomicUsize::new(0)));
    tools.register(tool.clone()).unwrap();
    let store = Arc::new(StopAfterApprovalStore::default());
    let first_runtime = AgentRuntime::new(
        "fake/test".into(),
        models.clone(),
        tools.clone(),
        store.clone(),
        Arc::new(FixedPermission(PermissionDecision::Ask)),
    )
    .unwrap();
    assert_eq!(
        first_runtime
            .submit("approval", Content::Text("start".into()))
            .await
            .unwrap(),
        TurnOutcome::ApprovalRequired(call)
    );
    assert!(
        first_runtime
            .resume("approval", "approved", true)
            .await
            .is_err()
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 0);
    drop(first_runtime);

    let second_runtime = AgentRuntime::new(
        "fake/test".into(),
        models,
        tools,
        store,
        Arc::new(FixedPermission(PermissionDecision::Ask)),
    )
    .unwrap();
    assert_eq!(
        second_runtime.continue_turn("approval").await.unwrap(),
        TurnOutcome::Completed(Content::Text("done".into()))
    );
    assert_eq!(tool.0.load(Ordering::SeqCst), 1);
}
