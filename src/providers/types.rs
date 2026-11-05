//! Normalized AI provider domain types.

use secrecy::SecretString;

/// Token accounting reported by a provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub total_tokens: u32,
}

impl TokenUsage {
    pub fn merge(&mut self, other: Self) {
        self.prompt_tokens = self.prompt_tokens.max(other.prompt_tokens);
        self.completion_tokens = self.completion_tokens.max(other.completion_tokens);
        self.cached_tokens = self.cached_tokens.max(other.cached_tokens);
        self.total_tokens = self
            .total_tokens
            .max(other.total_tokens)
            .max(self.prompt_tokens + self.completion_tokens);
    }
}

/// Why a completion finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolCalls,
    Cancelled,
    Other,
}

impl StopReason {
    pub fn from_wire(value: Option<&str>) -> Self {
        match value {
            Some("stop") | Some("end_turn") | Some("STOP") => Self::EndTurn,
            Some("length") | Some("max_tokens") | Some("MAX_TOKENS") => Self::MaxTokens,
            Some("tool_calls") | Some("tool_use") => Self::ToolCalls,
            Some("stop_sequence") => Self::StopSequence,
            Some(_) => Self::Other,
            None => Self::EndTurn,
        }
    }
}

/// Streaming events emitted to the AI coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    UsageUpdate(TokenUsage),
    Completed(StopReason),
}

/// Capabilities of a model/endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub supports_streaming: bool,
    pub supports_fim_completion: bool,
    pub supports_system_prompt: bool,
    pub supports_reasoning_effort: bool,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            max_output_tokens: 8_192,
            supports_streaming: true,
            supports_fim_completion: false,
            supports_system_prompt: true,
            supports_reasoning_effort: false,
        }
    }
}

/// A discoverable model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
}

/// Chat role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_openai(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn as_anthropic(self) -> &'static str {
        match self {
            Self::System => "user",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn as_gemini(self) -> &'static str {
        match self {
            Self::System => "user",
            Self::User => "user",
            Self::Assistant => "model",
        }
    }
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Completion request normalized across providers.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    /// Fill-in-the-middle prefix/suffix for inline completions.
    pub fim: Option<FimRequest>,
}

impl CompletionRequest {
    pub fn chat(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            system: None,
            max_tokens: None,
            temperature: None,
            fim: None,
        }
    }
}

/// Fill-in-the-middle request used for inline ghost text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FimRequest {
    pub prefix: String,
    pub suffix: String,
}

/// Supported provider families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Gemini,
    Ollama,
}

impl ProviderKind {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "openai" | "openrouter" | "deepseek" | "groq" | "llamacpp" | "lmstudio" => {
                Some(Self::OpenAi)
            }
            "anthropic" | "claude" => Some(Self::Anthropic),
            "gemini" | "google" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    pub fn default_endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::Ollama => "http://127.0.0.1:11434",
        }
    }

    pub fn default_env_var(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::Gemini => Some("GEMINI_API_KEY"),
            Self::Ollama => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI-compatible",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Google Gemini",
            Self::Ollama => "Ollama",
        }
    }

    pub fn default_capabilities(self) -> ModelCapabilities {
        match self {
            Self::Ollama => ModelCapabilities {
                context_window: 32_768,
                max_output_tokens: 4_096,
                supports_fim_completion: true,
                ..ModelCapabilities::default()
            },
            _ => ModelCapabilities::default(),
        }
    }
}

/// Provider construction settings. Secrets never enter configuration files.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model: Option<String>,
    pub api_key: Option<SecretString>,
}

impl ProviderConfig {
    pub fn new(id: impl Into<String>, kind: ProviderKind) -> Self {
        Self {
            id: id.into(),
            kind,
            endpoint: kind.default_endpoint().to_string(),
            model: None,
            api_key: None,
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_api_key(mut self, key: Option<SecretString>) -> Self {
        self.api_key = key;
        self
    }

    pub fn base_url(&self) -> &str {
        self.endpoint.trim_end_matches('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_aliases_resolve() {
        assert_eq!(
            ProviderKind::from_id("openrouter"),
            Some(ProviderKind::OpenAi)
        );
        assert_eq!(
            ProviderKind::from_id("claude"),
            Some(ProviderKind::Anthropic)
        );
        assert_eq!(ProviderKind::from_id("unknown"), None);
    }

    #[test]
    fn usage_merge_keeps_maximums() {
        let mut usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            ..Default::default()
        };
        usage.merge(TokenUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
            ..Default::default()
        });
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 17);
    }

    #[test]
    fn stop_reason_maps_wire_values() {
        assert_eq!(StopReason::from_wire(Some("length")), StopReason::MaxTokens);
        assert_eq!(
            StopReason::from_wire(Some("tool_use")),
            StopReason::ToolCalls
        );
        assert_eq!(StopReason::from_wire(None), StopReason::EndTurn);
    }
}
