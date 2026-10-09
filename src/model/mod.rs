pub mod provider;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use provider::{ModelContext, ModelProvider, ModelResponse, ToolDefinition};

#[derive(Default)]
pub struct ModelRegistry {
    providers: RwLock<HashMap<String, Arc<dyn ModelProvider>>>,
}

impl ModelRegistry {
    pub fn register(&self, provider: Arc<dyn ModelProvider>) -> Result<()> {
        let mut providers = self
            .providers
            .write()
            .expect("model registry lock poisoned");
        let name = provider.name().to_owned();
        if providers.contains_key(&name) {
            bail!("model provider already registered: {name}");
        }
        providers.insert(name, provider);
        Ok(())
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let mut providers: Vec<_> = self
            .providers
            .read()
            .expect("model registry lock poisoned")
            .iter()
            .map(|(name, provider)| (name.clone(), provider.clone()))
            .collect();
        providers.sort_by(|a, b| a.0.cmp(&b.0));
        let mut models = Vec::new();
        for (name, provider) in providers {
            let available = provider
                .model_list()
                .await
                .with_context(|| format!("failed to list models from provider: {name}"))?;
            models.extend(available.into_iter().map(|model| format!("{name}/{model}")));
        }
        models.sort();
        Ok(models)
    }

    pub async fn invoke(
        &self,
        selected_model: &str,
        context: &ModelContext,
        tools: &[ToolDefinition],
    ) -> Result<ModelResponse> {
        let (provider_name, model_name) = selected_model
            .split_once('/')
            .context("model must have the form provider/model")?;
        if provider_name.is_empty() || model_name.is_empty() {
            bail!("model must have the form provider/model");
        }
        let provider = self
            .providers
            .read()
            .expect("model registry lock poisoned")
            .get(provider_name)
            .cloned()
            .with_context(|| format!("model provider is not registered: {provider_name}"))?;
        provider.invoke(model_name, context, tools).await
    }
}
