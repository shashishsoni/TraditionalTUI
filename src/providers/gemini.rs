//! Google Gemini `streamGenerateContent` adapter.

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    AiProvider, CompletionRequest, ModelCapabilities, ModelInfo, ProviderConfig, ProviderError,
    Role, StopReason, StreamEvent, TokenUsage, ensure_success, http_client, sse,
};

/// Gemini Generative Language API adapter.
pub struct GeminiProvider {
    id: String,
    config: ProviderConfig,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let client = http_client()?;
        Ok(Self {
            id: config.id.clone(),
            config,
            client,
        })
    }

    fn key(&self) -> Result<&secrecy::SecretString, ProviderError> {
        self.config
            .api_key
            .as_ref()
            .ok_or_else(|| ProviderError::MissingKey(self.config.id.clone()))
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Google Gemini"
    }

    async fn list_models(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let key = self.key()?;
        let url = format!("{}/v1beta/models", self.config.base_url());
        let request = self
            .client
            .get(url)
            .header("x-goog-api-key", key.expose_secret());
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            response = request.send() => response?,
        };
        let response = ensure_success(response).await?;
        let value: Value = response.json().await?;
        let models = value
            .get("models")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let name = item.get("name")?.as_str()?;
                        let id = name.strip_prefix("models/").unwrap_or(name).to_string();
                        let display_name = item
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or(&id)
                            .to_string();
                        let capabilities = ModelCapabilities {
                            context_window: item
                                .get("inputTokenLimit")
                                .and_then(Value::as_u64)
                                .unwrap_or(1_000_000)
                                as usize,
                            max_output_tokens: item
                                .get("outputTokenLimit")
                                .and_then(Value::as_u64)
                                .unwrap_or(8_192)
                                as usize,
                            ..ModelCapabilities::default()
                        };
                        Some(ModelInfo {
                            id,
                            display_name,
                            capabilities,
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
        let key = self.key()?;
        let mut contents = Vec::new();
        for message in &request.messages {
            if message.role == Role::System {
                continue;
            }
            contents.push(json!({
                "role": message.role.as_gemini(),
                "parts": [{ "text": message.content }],
            }));
        }

        let mut body = json!({ "contents": contents });
        if let Some(system) = &request.system {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }
        let mut generation = serde_json::Map::new();
        if let Some(max_tokens) = request.max_tokens {
            generation.insert("maxOutputTokens".into(), json!(max_tokens));
        }
        if let Some(temperature) = request.temperature {
            generation.insert("temperature".into(), json!(temperature));
        }
        if !generation.is_empty() {
            body["generationConfig"] = Value::Object(generation);
        }

        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.config.base_url(),
            request.model
        );
        let request_builder = self
            .client
            .post(url)
            .header("x-goog-api-key", key.expose_secret())
            .json(&body);
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
            let Some(value) = sse::json_event(&event.data) else {
                continue;
            };
            for stream_event in parse_payload(&value) {
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
        Ok(ModelCapabilities {
            context_window: 1_000_000,
            max_output_tokens: 8_192,
            ..ModelCapabilities::default()
        })
    }
}

/// Parse a Gemini SSE payload (object or array of chunks).
pub fn parse_payload(value: &Value) -> Vec<StreamEvent> {
    match value {
        Value::Array(items) => items.iter().flat_map(parse_chunk).collect(),
        other => parse_chunk(other),
    }
}

/// Parse one Gemini response chunk.
pub fn parse_chunk(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    if let Some(candidates) = value.get("candidates").and_then(Value::as_array) {
        for candidate in candidates {
            if let Some(parts) = candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
            {
                for part in parts {
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    if text.is_empty() {
                        continue;
                    }
                    if part.get("thought").and_then(Value::as_bool) == Some(true) {
                        events.push(StreamEvent::ReasoningDelta(text.to_string()));
                    } else {
                        events.push(StreamEvent::TextDelta(text.to_string()));
                    }
                }
            }
        }
    }
    if let Some(usage) = value.get("usageMetadata") {
        let prompt_tokens = usage
            .get("promptTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let completion_tokens = usage
            .get("candidatesTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let total_tokens = usage
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or((prompt_tokens + completion_tokens) as u64)
            as u32;
        events.push(StreamEvent::UsageUpdate(TokenUsage {
            prompt_tokens,
            completion_tokens,
            cached_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            total_tokens,
        }));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_thought_and_usage() {
        let chunk = json!({
            "candidates": [{
                "content": { "parts": [
                    { "text": "answer" },
                    { "text": "reason", "thought": true }
                ]},
            }],
            "usageMetadata": {
                "promptTokenCount": 3,
                "candidatesTokenCount": 2,
                "totalTokenCount": 5
            }
        });
        let events = parse_chunk(&chunk);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, StreamEvent::TextDelta(text) if text == "answer"))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, StreamEvent::ReasoningDelta(_)))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            StreamEvent::UsageUpdate(usage) if usage.total_tokens == 5
        )));
    }

    #[test]
    fn array_payloads_are_flattened() {
        let payload = json!([
            {"candidates": [{"content": {"parts": [{"text": "a"}]}}]},
            {"candidates": [{"content": {"parts": [{"text": "b"}]}}]}
        ]);
        let texts: Vec<_> = parse_payload(&payload)
            .into_iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["a", "b"]);
    }
}
