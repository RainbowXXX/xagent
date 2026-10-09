use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum Content {
    Text(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ToolResult {
    pub call_id: String,
    pub content: Content,
    pub is_error: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ModelInput {
    UserInput(Content),
    ExtensionInput { source: String, content: Content },
    ToolResult(ToolResult),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ModelOutput {
    Chat(Content),
    Reasoning(Content),
    ToolCall(ToolCall),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ModelContextUnit {
    Input(ModelInput),
    Output(ModelOutput),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ModelContext {
    pub units: Vec<ModelContextUnit>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ModelResponse {
    pub outputs: Vec<ModelOutput>,
}

/// Providers translate these framework types to their own API format.
pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn model_list(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>>;

    fn invoke<'a>(
        &'a self,
        model: &'a str,
        context: &'a ModelContext,
        tools: &'a [ToolDefinition],
    ) -> BoxFuture<'a, anyhow::Result<ModelResponse>>;
}
