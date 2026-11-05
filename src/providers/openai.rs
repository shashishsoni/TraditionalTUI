//! OpenAI-compatible provider (OpenAI, OpenRouter, DeepSeek, Groq, vLLM,
//! llama.cpp, LM Studio).

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    AiProvider, CompletionRequest, ModelCapabilities, ModelInfo, ProviderConfig, ProviderError,
    StopReason, StreamEvent, TokenUsage, ensure_success, http_client, sse,
};

/// OpenAI chat-completions adapter.
pub struct OpenAiProvider {
    id: String,
    display_name: String,
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let client = http_client()?;
        Ok(Self {
            id: config.id.clone(),
            display_name: if config.id == "openai" {
                "OpenAI".to_string()
            } else {
                config.id.clone()
            },
            config,
            client,
        })
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.config.api_key {
            Some(key) => builder.bearer_auth(key.expose_secret()),
            None => builder,
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    async fn list_models(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/models", self.config.base_url());
        let request = self.authorized(self.client.get(url));
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            response = request.send() => response?,
        };
        let response = ensure_success(response).await?;
        let value: Value = response.json().await?;
        let models = value
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let id = item.get("id")?.as_str()?.to_string();
                        Some(ModelInfo {
                            display_name: id.clone(),
                            id,
                            capabilities: self.config.kind.default_capabilities(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<TokenUsage, ProviderError> {
        let url = format!("{}/chat/completions", self.config.base_url());
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.extend(request.messages.iter().map(
            |message| json!({ "role": message.role.as_openai(), "content": message.content }),
        ));

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }

        let request_builder = self.authorized(self.client.post(url).json(&body));
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            response = request_builder.send() => response?,
        };
        let response = ensure_success(response).await?;

        let mut stream = Box::pin(response.bytes_stream().eventsource());
        let mut usage = TokenUsage::default();
        loop {
            let item = tokio::select! {
                _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                item = stream.next() => item,
            };
            let Some(item) = item else {
                break;
            };
            let event = item.map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
            if event.data.trim() == "[DONE]" {
                break;
            }
            let Some(value) = sse::json_event(&event.data) else {
                continue;
            };
            for stream_event in parse_chunk(&value) {
                if let StreamEvent::UsageUpdate(update) = &stream_event {
                    usage.merge(*update);
                }
                if tx.send(stream_event).await.is_err() {
                    return Err(ProviderError::Cancelled);
                }
            }
        }
        let _ = tx.send(StreamEvent::Completed(StopReason::EndTurn)).await;
        Ok(usage)
    }

    async fn probe_capabilities(&self, _model: &str) -> Result<ModelCapabilities, ProviderError> {
        Ok(self.config.kind.default_capabilities())
    }
}

/// Parse one streaming chunk from the chat-completions SSE payload.
pub fn parse_chunk(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(text) = choice
                .pointer("/delta/content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                events.push(StreamEvent::TextDelta(text.to_string()));
            }
            if let Some(text) = choice
                .pointer("/delta/reasoning_content")
                .and_then(Value::as_str)
                .or_else(|| choice.pointer("/delta/reasoning").and_then(Value::as_str))
                .filter(|text| !text.is_empty())
            {
                events.push(StreamEvent::ReasoningDelta(text.to_string()));
            }
        }
    }
    if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
        events.push(StreamEvent::UsageUpdate(parse_usage(usage)));
    }
    events
}

fn parse_usage(value: &Value) -> TokenUsage {
    let prompt_tokens = value
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let completion_tokens = value
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let cached_tokens = value
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let total_tokens = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or((prompt_tokens + completion_tokens) as u64) as u32;
    TokenUsage {
        prompt_tokens,
        completion_tokens,
        cached_tokens,
        total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_text_reasoning_and_usage() {
        let chunk = json!({
            "choices": [{
                "delta": { "content": "hello", "reasoning_content": "thinking" },
                "finish_reason": null
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12,
                       "prompt_tokens_details": { "cached_tokens": 4 } }
        });
        let events = parse_chunk(&chunk);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, StreamEvent::TextDelta(text) if text == "hello"))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, StreamEvent::ReasoningDelta(_)))
        );
        let usage = events
            .iter()
            .find_map(|event| match event {
                StreamEvent::UsageUpdate(usage) => Some(*usage),
                _ => None,
            })
            .expect("usage");
        assert_eq!(usage.cached_tokens, 4);
        assert_eq!(usage.total_tokens, 12);
    }

    #[test]
    fn ignores_null_usage_and_empty_deltas() {
        let chunk = json!({"choices": [{"delta": {}}], "usage": null});
        assert!(parse_chunk(&chunk).is_empty());
    }
}
