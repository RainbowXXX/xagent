pub enum Content {
    Text(String),
}

enum ModelInput {
    UserInput(Content),

    ToolResult(Content),
}

enum ModelOutput {
    Chat(Content),
    Reasoning(Content),

    ToolCall(),
}

pub enum ModelContextUnit {
    Input(ModelInput),
    Output(ModelOutput),
}

pub struct ModelContext {
    context: Vec<ModelContextUnit>,
}

pub trait ModelProvider {
    type ProviderError;
    type ModelDescriptor;

    // name of provider
    fn name(&self) -> &'static str;

    async fn model_list(&self) -> Result<Vec<ModelDescriptor>, Self::ProviderError>;

    // invoke the model
    async fn invoke(&self, descriptor: ModelDescriptor, context: ModelContext) -> Result<ModelResponse, Self::ProviderError>;
}
