use crate::model::provider::{
    BoxFuture, ModelContext, ModelProvider, ModelResponse, ToolDefinition,
};

/// Adapter placeholder; wire an API client here when provider work begins.
pub struct OpenAIProvider;

impl ModelProvider for OpenAIProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn model_list(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>> {
        Box::pin(async { anyhow::bail!("OpenAI provider is not implemented") })
    }

    fn invoke<'a>(
        &'a self,
        _model: &'a str,
        _context: &'a ModelContext,
        _tools: &'a [ToolDefinition],
    ) -> BoxFuture<'a, anyhow::Result<ModelResponse>> {
        Box::pin(async { anyhow::bail!("OpenAI provider is not implemented") })
    }
}
