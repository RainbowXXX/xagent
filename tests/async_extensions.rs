use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use tokio::sync::oneshot;
use tokio::time::timeout;

use xagent::model::ModelRegistry;
use xagent::model::provider::{
    BoxFuture, Content, ModelContext, ModelContextUnit, ModelInput, ModelOutput, ModelProvider,
    ModelResponse, ToolCall, ToolDefinition,
};
use xagent::permission::{PermissionDecision, PermissionEngine};
use xagent::runtime::{
    AgentRuntime, ExtensionContext, RuntimeEvent, RuntimeExtension, RuntimeHandle, TurnOutcome,
};
use xagent::session::{InMemorySessionStore, Session, SessionStatus};
use xagent::tool::ToolRegistry;

struct DenyTools;

impl PermissionEngine for DenyTools {
    fn decide<'a>(
        &'a self,
        _session: &'a Session,
        _call: &'a ToolCall,
    ) -> BoxFuture<'a, Result<PermissionDecision>> {
        Box::pin(async { Ok(PermissionDecision::Deny) })
    }
}

struct WaitingProvider {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    calls: AtomicUsize,
    contexts: Mutex<Vec<ModelContext>>,
}

impl WaitingProvider {
    fn new(entered: oneshot::Sender<()>, release: oneshot::Receiver<()>) -> Self {
        Self {
            entered: Mutex::new(Some(entered)),
            release: Mutex::new(Some(release)),
            calls: AtomicUsize::new(0),
            contexts: Mutex::new(Vec::new()),
        }
    }
}

impl ModelProvider for WaitingProvider {
    fn name(&self) -> &'static str {
        "waiting"
    }

    fn model_list(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(vec!["model".into()]) })
    }

    fn invoke<'a>(
        &'a self,
        _model: &'a str,
        context: &'a ModelContext,
        _tools: &'a [ToolDefinition],
    ) -> BoxFuture<'a, Result<ModelResponse>> {
        self.contexts.lock().unwrap().push(context.clone());
        let step = self.calls.fetch_add(1, Ordering::SeqCst);
        if step == 0 {
            let entered = self.entered.lock().unwrap().take().unwrap();
            let release = self.release.lock().unwrap().take().unwrap();
            Box::pin(async move {
                let _ = entered.send(());
                release.await?;
                Ok(ModelResponse {
                    outputs: vec![ModelOutput::Chat(Content::Text("first".into()))],
                })
            })
        } else {
            Box::pin(async {
                Ok(ModelResponse {
                    outputs: vec![ModelOutput::Chat(Content::Text("second".into()))],
                })
            })
        }
    }
}

struct ImmediateProvider;

impl ModelProvider for ImmediateProvider {
    fn name(&self) -> &'static str {
        "immediate"
    }

    fn model_list(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async { Ok(vec!["model".into()]) })
    }

    fn invoke<'a>(
        &'a self,
        _model: &'a str,
        context: &'a ModelContext,
        _tools: &'a [ToolDefinition],
    ) -> BoxFuture<'a, Result<ModelResponse>> {
        let initiated_by_extension = context.units.iter().any(|unit| matches!(unit,
            ModelContextUnit::Input(ModelInput::ExtensionInput { source, .. }) if source == "watcher"
        ));
        Box::pin(async move {
            assert!(initiated_by_extension);
            Ok(ModelResponse {
                outputs: vec![ModelOutput::Chat(Content::Text("started".into()))],
            })
        })
    }
}

#[derive(Debug, PartialEq)]
enum DeferredAction {
    Queued,
    Started(TurnOutcome),
}

struct DeferredExtension {
    session_id: String,
    trigger: Mutex<Option<oneshot::Receiver<()>>>,
    done: Mutex<Option<oneshot::Sender<Result<DeferredAction>>>>,
}

#[derive(Default)]
struct RespondedExtension {
    context: Mutex<Option<ExtensionContext>>,
    posted: std::sync::atomic::AtomicBool,
}

impl RuntimeExtension for RespondedExtension {
    fn on_attach(&self, context: ExtensionContext) -> BoxFuture<'_, Result<()>> {
        *self.context.lock().unwrap() = Some(context);
        Box::pin(async { Ok(()) })
    }

    fn on_event(&self, event: RuntimeEvent) -> BoxFuture<'_, ()> {
        if matches!(event, RuntimeEvent::ModelResponded { .. })
            && !self.posted.swap(true, Ordering::SeqCst)
        {
            self.context
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .post_context(
                    "responding",
                    "event-hook",
                    Content::Text("late input".into()),
                )
                .unwrap();
        }
        Box::pin(async {})
    }
}

impl RuntimeExtension for DeferredExtension {
    fn on_attach(&self, context: ExtensionContext) -> BoxFuture<'_, Result<()>> {
        let trigger = self.trigger.lock().unwrap().take().unwrap();
        let done = self.done.lock().unwrap().take().unwrap();
        let session_id = self.session_id.clone();
        let background = context.clone();
        context.spawn(async move {
            let result = async {
                trigger.await?;
                let status = background
                    .session(&session_id)?
                    .map(|session| session.status);
                if status == Some(SessionStatus::Running) {
                    background.post_context(
                        &session_id,
                        "watcher",
                        Content::Text("file appeared".into()),
                    )?;
                    Ok(DeferredAction::Queued)
                } else {
                    let task = background.start_turn(
                        &session_id,
                        "watcher",
                        Content::Text("file appeared".into()),
                    )?;
                    Ok(DeferredAction::Started(task.await??))
                }
            }
            .await;
            let _ = done.send(result);
        });
        Box::pin(async { Ok(()) })
    }
}

fn deferred_extension(
    session_id: &str,
) -> (
    Arc<DeferredExtension>,
    oneshot::Sender<()>,
    oneshot::Receiver<Result<DeferredAction>>,
) {
    let (trigger_tx, trigger_rx) = oneshot::channel();
    let (done_tx, done_rx) = oneshot::channel();
    (
        Arc::new(DeferredExtension {
            session_id: session_id.into(),
            trigger: Mutex::new(Some(trigger_rx)),
            done: Mutex::new(Some(done_tx)),
        }),
        trigger_tx,
        done_rx,
    )
}

fn runtime(provider: Arc<dyn ModelProvider>, model: &str) -> Arc<AgentRuntime> {
    let models = Arc::new(ModelRegistry::default());
    models.register(provider).unwrap();
    Arc::new(
        AgentRuntime::new(
            model.into(),
            models,
            Arc::new(ToolRegistry::default()),
            Arc::new(InMemorySessionStore::default()),
            Arc::new(DenyTools),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn extension_can_start_a_turn_after_agent_becomes_idle() {
    let runtime = runtime(Arc::new(ImmediateProvider), "immediate/model");
    let (extension, trigger, done) = deferred_extension("idle");
    let handle: Arc<dyn RuntimeHandle> = runtime.clone();
    handle.attach_extension(extension).await.unwrap();
    trigger.send(()).unwrap();

    let action = timeout(Duration::from_secs(2), done)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        action,
        DeferredAction::Started(TurnOutcome::Completed(Content::Text("started".into())))
    );
    assert_eq!(
        runtime.session("idle").unwrap().unwrap().status,
        SessionStatus::Idle
    );
}

#[tokio::test]
async fn extension_context_arriving_during_model_call_reaches_next_step() {
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let provider = Arc::new(WaitingProvider::new(entered_tx, release_rx));
    let runtime = runtime(provider.clone(), "waiting/model");
    let (extension, trigger, done) = deferred_extension("running");
    AgentRuntime::attach_extension(&runtime, extension)
        .await
        .unwrap();

    let active_runtime = runtime.clone();
    let turn = tokio::spawn(async move {
        active_runtime
            .submit("running", Content::Text("user message".into()))
            .await
    });
    timeout(Duration::from_secs(2), entered_rx)
        .await
        .unwrap()
        .unwrap();
    trigger.send(()).unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), done)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        DeferredAction::Queued
    );
    release_tx.send(()).unwrap();

    assert_eq!(
        timeout(Duration::from_secs(2), turn)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        TurnOutcome::Completed(Content::Text("second".into()))
    );
    let contexts = provider.contexts.lock().unwrap();
    assert_eq!(contexts.len(), 2);
    assert!(contexts[1].units.iter().any(|unit| matches!(unit,
        ModelContextUnit::Input(ModelInput::ExtensionInput { source, content: Content::Text(text) })
            if source == "watcher" && text == "file appeared"
    )));
}

#[tokio::test]
async fn extension_can_post_context_from_model_response_event() {
    let (entered_tx, _entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    release_tx.send(()).unwrap();
    let provider = Arc::new(WaitingProvider::new(entered_tx, release_rx));
    let runtime = runtime(provider.clone(), "waiting/model");
    AgentRuntime::attach_extension(&runtime, Arc::new(RespondedExtension::default()))
        .await
        .unwrap();

    assert_eq!(
        runtime
            .submit("responding", Content::Text("start".into()))
            .await
            .unwrap(),
        TurnOutcome::Completed(Content::Text("second".into()))
    );
    let contexts = provider.contexts.lock().unwrap();
    assert_eq!(contexts.len(), 2);
    assert!(contexts[1].units.iter().any(|unit| matches!(unit,
        ModelContextUnit::Input(ModelInput::ExtensionInput { source, .. }) if source == "event-hook"
    )));
}
