//! AI conversation orchestration for BYOK providers.
//!
//! The coordinator owns chat state and spawns streaming completions on the
//! Tokio runtime; events flow back to the app through a typed channel.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::providers::{AiProvider, ChatMessage, CompletionRequest, StreamEvent, TokenUsage};

/// Coordinator failure.
#[derive(Debug, Error)]
pub enum AiError {
    #[error("no BYOK provider is configured")]
    NotConfigured,

    #[error("an AI turn is already in progress")]
    Busy,
}

/// Chat message role in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

/// One rendered conversation entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEntry {
    pub role: ChatRole,
    pub content: String,
}

/// A conversation thread.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatThread {
    entries: Vec<ChatEntry>,
    title: String,
}

impl ChatThread {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            title: title.into(),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn entries(&self) -> &[ChatEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, role: ChatRole, content: impl Into<String>) {
        self.entries.push(ChatEntry {
            role,
            content: content.into(),
        });
    }

    /// Append streamed text to the trailing assistant entry, creating one.
    pub fn append_assistant(&mut self, text: &str) {
        match self.entries.last_mut() {
            Some(entry) if entry.role == ChatRole::Assistant => entry.content.push_str(text),
            _ => self.push(ChatRole::Assistant, text),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Messages in provider-neutral form.
    pub fn messages(&self) -> Vec<ChatMessage> {
        self.entries
            .iter()
            .map(|entry| match entry.role {
                ChatRole::User => ChatMessage::user(entry.content.clone()),
                ChatRole::Assistant => ChatMessage::assistant(entry.content.clone()),
            })
            .collect()
    }

    pub fn last_assistant(&self) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.role == ChatRole::Assistant)
            .map(|entry| entry.content.as_str())
    }
}

/// Coordinator lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiState {
    Disabled,
    Idle,
    Streaming,
    Error,
}

/// Events emitted to the app while a turn streams.
#[derive(Debug, Clone)]
pub enum AiEvent {
    Text(String),
    Reasoning(String),
    Completed(TokenUsage),
    Error(String),
}

/// Owns the active provider and conversation.
pub struct AiCoordinator {
    provider: Option<Arc<dyn AiProvider>>,
    model: String,
    thread: ChatThread,
    state: AiState,
    last_usage: TokenUsage,
}

impl Default for AiCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AiCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AiCoordinator")
            .field(
                "provider",
                &self.provider.as_ref().map(|provider| provider.id()),
            )
            .field("model", &self.model)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl AiCoordinator {
    /// Zero-AI coordinator with no provider and no background work.
    pub fn new() -> Self {
        Self {
            provider: None,
            model: String::new(),
            thread: ChatThread::new("Assistant"),
            state: AiState::Disabled,
            last_usage: TokenUsage::default(),
        }
    }

    /// Attach a provider and model.
    pub fn configure(&mut self, provider: Arc<dyn AiProvider>, model: impl Into<String>) {
        self.provider = Some(provider);
        self.model = model.into();
        self.state = AiState::Idle;
    }

    pub fn is_configured(&self) -> bool {
        self.provider.is_some()
    }

    pub fn provider_name(&self) -> Option<&str> {
        self.provider
            .as_ref()
            .map(|provider| provider.display_name())
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn state(&self) -> AiState {
        self.state
    }

    pub fn thread(&self) -> &ChatThread {
        &self.thread
    }

    pub fn thread_mut(&mut self) -> &mut ChatThread {
        &mut self.thread
    }

    pub fn last_usage(&self) -> TokenUsage {
        self.last_usage
    }

    /// Start a streaming turn. Results arrive on `events`.
    pub fn start_turn(
        &mut self,
        prompt: String,
        context: Option<String>,
        events: mpsc::Sender<AiEvent>,
        cancel: CancellationToken,
    ) -> Result<(), AiError> {
        let provider = self.provider.clone().ok_or(AiError::NotConfigured)?;
        if self.state == AiState::Streaming {
            return Err(AiError::Busy);
        }

        self.thread.push(ChatRole::User, prompt);
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: self.thread.messages(),
            system: context,
            max_tokens: None,
            temperature: Some(0.2),
            fim: None,
        };
        self.state = AiState::Streaming;

        tokio::spawn(async move {
            let (stream_tx, mut stream_rx) = mpsc::channel(128);
            let provider_task = {
                let provider = provider.clone();
                let cancel = cancel.clone();
                tokio::spawn(
                    async move { provider.stream_completion(request, stream_tx, cancel).await },
                )
            };

            while let Some(event) = stream_rx.recv().await {
                let forwarded = match event {
                    StreamEvent::TextDelta(text) => AiEvent::Text(text),
                    StreamEvent::ReasoningDelta(text) => AiEvent::Reasoning(text),
                    StreamEvent::UsageUpdate(_) => continue,
                    StreamEvent::Completed(_) => continue,
                };
                if events.send(forwarded).await.is_err() {
                    cancel.cancel();
                    break;
                }
            }

            match provider_task.await {
                Ok(Ok(usage)) => {
                    let _ = events.send(AiEvent::Completed(usage)).await;
                }
                Ok(Err(error)) => {
                    let _ = events.send(AiEvent::Error(error.to_string())).await;
                }
                Err(error) => {
                    let _ = events.send(AiEvent::Error(error.to_string())).await;
                }
            }
        });
        Ok(())
    }

    /// Apply a streamed event to conversation state.
    pub fn apply_event(&mut self, event: &AiEvent) {
        match event {
            AiEvent::Text(text) => {
                self.thread.append_assistant(text);
                self.state = AiState::Streaming;
            }
            AiEvent::Reasoning(_) => {}
            AiEvent::Completed(usage) => {
                self.last_usage = *usage;
                self.state = AiState::Idle;
            }
            AiEvent::Error(message) => {
                self.thread
                    .push(ChatRole::Assistant, format!("[error] {message}"));
                self.state = AiState::Error;
            }
        }
    }

    /// Mark a cancelled turn as idle.
    pub fn cancelled(&mut self) {
        self.state = AiState::Idle;
    }

    pub fn clear(&mut self) {
        self.thread.clear();
        self.state = if self.provider.is_some() {
            AiState::Idle
        } else {
            AiState::Disabled
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_coordinator_is_disabled() {
        let coordinator = AiCoordinator::new();
        assert!(!coordinator.is_configured());
        assert_eq!(coordinator.state(), AiState::Disabled);
    }

    #[test]
    fn streaming_text_appends_to_assistant_entry() {
        let mut coordinator = AiCoordinator::new();
        coordinator.apply_event(&AiEvent::Text("hello ".into()));
        coordinator.apply_event(&AiEvent::Text("world".into()));
        assert_eq!(coordinator.thread().last_assistant(), Some("hello world"));
        coordinator.apply_event(&AiEvent::Completed(TokenUsage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            cached_tokens: 0,
        }));
        assert_eq!(coordinator.state(), AiState::Idle);
        assert_eq!(coordinator.last_usage().total_tokens, 3);
    }

    #[test]
    fn start_without_provider_fails() {
        let mut coordinator = AiCoordinator::new();
        let (tx, _rx) = mpsc::channel(4);
        assert!(matches!(
            coordinator.start_turn("hi".into(), None, tx, CancellationToken::new()),
            Err(AiError::NotConfigured)
        ));
    }

    #[test]
    fn thread_messages_preserve_roles() {
        let mut thread = ChatThread::new("t");
        thread.push(ChatRole::User, "q");
        thread.push(ChatRole::Assistant, "a");
        let messages = thread.messages();
        assert_eq!(messages[0].role, crate::providers::Role::User);
        assert_eq!(messages[1].role, crate::providers::Role::Assistant);
    }
}
