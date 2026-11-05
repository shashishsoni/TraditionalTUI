//! Bring Your Own Key (BYOK) LLM provider layer.
//!
//! Providers are stateless HTTPS/SSE clients and are intentionally decoupled
//! from ACP agents. Nothing here runs at startup: providers are constructed on
//! demand by the AI coordinator, and only when a key is configured.

pub mod anthropic;
pub mod credentials;
pub mod error;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod sse;
pub mod types;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub use credentials::CredentialStore;
pub use error::ProviderError;
pub use types::{
    ChatMessage, CompletionRequest, FimRequest, ModelCapabilities, ModelInfo, ProviderConfig,
    ProviderKind, Role, StopReason, StreamEvent, TokenUsage,
};

/// Provider-independent completion interface.
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Unique identifier (`openai`, `anthropic`, `gemini`, `ollama`).
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Discover models exposed by the endpoint.
    async fn list_models(&self, cancel: CancellationToken)
    -> Result<Vec<ModelInfo>, ProviderError>;

    /// Stream a completion into `tx` and return final token usage.
    async fn stream_completion(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<TokenUsage, ProviderError>;

    /// Probe capabilities for a model.
    async fn probe_capabilities(&self, model: &str) -> Result<ModelCapabilities, ProviderError>;
}

/// Construct a provider from normalized configuration.
pub fn provider_from_config(config: ProviderConfig) -> Result<Box<dyn AiProvider>, ProviderError> {
    sse::validate_endpoint(config.base_url())?;
    let provider: Box<dyn AiProvider> = match config.kind {
        ProviderKind::OpenAi => Box::new(openai::OpenAiProvider::new(config)?),
        ProviderKind::Anthropic => Box::new(anthropic::AnthropicProvider::new(config)?),
        ProviderKind::Gemini => Box::new(gemini::GeminiProvider::new(config)?),
        ProviderKind::Ollama => Box::new(ollama::OllamaProvider::new(config)?),
    };
    Ok(provider)
}

pub(crate) fn http_client() -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .user_agent(format!("ttui/{}", crate::VERSION))
        .build()
        .map_err(ProviderError::Http)
}

pub(crate) async fn ensure_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(ProviderError::Status {
        status: status.as_u16(),
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_rejects_plaintext_remote_endpoints() {
        let config = ProviderConfig::new("openai", ProviderKind::OpenAi)
            .with_endpoint("http://example.com/v1");
        assert!(matches!(
            provider_from_config(config),
            Err(ProviderError::InvalidEndpoint(_))
        ));
    }

    #[test]
    fn factory_accepts_loopback_ollama() {
        let config = ProviderConfig::new("ollama", ProviderKind::Ollama)
            .with_endpoint("http://127.0.0.1:11434");
        let provider = provider_from_config(config).expect("provider");
        assert_eq!(provider.id(), "ollama");
    }
}
