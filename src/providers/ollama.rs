//! Ollama local daemon adapter (chat + fill-in-the-middle).

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    AiProvider, CompletionRequest, ModelCapabilities, ModelInfo, ProviderConfig, ProviderError,
    StopReason, StreamEvent, TokenUsage, ensure_success, http_client, sse,
};

/// Ollama native API adapter.
pub struct OllamaProvider {
    id: String,
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        let client = http_client()?;
        Ok(Self {
            id: config.id.clone(),
            config,
            client,
        })
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Ollama (local)"
    }

    async fn list_models(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/api/tags", self.config.base_url());
        let request = self.client.get(url);
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
                        let name = item
                            .get("name")
                            .or_else(|| item.get("model"))
                            .and_then(Value::as_str)?
                            .to_string();
                        Some(ModelInfo {
                            id: name.clone(),
                            display_name: name,
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
        let (url, body) = match &request.fim {
            Some(fim) => (
                format!("{}/api/generate", self.config.base_url()),
                json!({
                    "model": request.model,
                    "prompt": fim.prefix,
                    "suffix": fim.suffix,
                    "stream": true,
                }),
            ),
            None => {
                let messages: Vec<Value> = request
                    .messages
                    .iter()
                    .map(|message| {
                        json!({
                            "role": message.role.as_openai(),
                            "content": message.content,
                        })
                    })
                    .collect();
                (
                    format!("{}/api/chat", self.config.base_url()),
                    json!({
                        "model": request.model,
                        "messages": messages,
                        "stream": true,
                    }),
                )
            }
        };

        let request_builder = self.client.post(url).json(&body);
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            response = request_builder.send() => response?,
        };
        let response = ensure_success(response).await?;

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut usage = TokenUsage::default();
        let mut finished = false;
        while !finished {
            let item = tokio::select! {
                _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
                item = stream.next() => item,
            };
            let Some(item) = item else {
                break;
            };
            let bytes = item?;
            buffer.extend_from_slice(&bytes);
            for line in sse::take_lines(&mut buffer) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("done").and_then(Value::as_bool) == Some(true) {
                    finished = true;
                }
                for stream_event in parse_line(&value) {
                    if let StreamEvent::UsageUpdate(update) = &stream_event {
                        usage.merge(*update);
                    }
                    if tx.send(stream_event).await.is_err() {
                        return Err(ProviderError::Cancelled);
                    }
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

/// Parse one NDJSON line from `/api/chat` or `/api/generate`.
pub fn parse_line(value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    let text = value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .or_else(|| value.get("response").and_then(Value::as_str));
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        events.push(StreamEvent::TextDelta(text.to_string()));
    }

    if value.get("done").and_then(Value::as_bool) == Some(true) {
        let prompt_tokens = value
            .get("prompt_eval_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let completion_tokens = value.get("eval_count").and_then(Value::as_u64).unwrap_or(0) as u32;
        if prompt_tokens > 0 || completion_tokens > 0 {
            events.push(StreamEvent::UsageUpdate(TokenUsage {
                prompt_tokens,
                completion_tokens,
                cached_tokens: 0,
                total_tokens: prompt_tokens + completion_tokens,
            }));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chat_chunks_and_usage() {
        let chunk = json!({
            "message": { "role": "assistant", "content": "hi" },
            "done": false
        });
        assert_eq!(
            parse_line(&chunk),
            vec![StreamEvent::TextDelta("hi".into())]
        );

        let final_chunk = json!({
            "done": true,
            "prompt_eval_count": 7,
            "eval_count": 3
        });
        assert_eq!(
            parse_line(&final_chunk),
            vec![StreamEvent::UsageUpdate(TokenUsage {
                prompt_tokens: 7,
                completion_tokens: 3,
                total_tokens: 10,
                cached_tokens: 0,
            })]
        );
    }

    #[test]
    fn parses_generate_response_text() {
        let chunk = json!({ "response": "code", "done": false });
        assert_eq!(
            parse_line(&chunk),
            vec![StreamEvent::TextDelta("code".into())]
        );
    }
}
