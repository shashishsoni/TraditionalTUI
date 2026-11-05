//! Anthropic Messages API adapter with SSE streaming.

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

/// Anthropic `/v1/messages` adapter.
pub struct AnthropicProvider {
    id: String,
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicProvider {
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
impl AiProvider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        "Anthropic"
    }

    async fn list_models(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        Ok(self
            .config
            .model
            .as_ref()
            .map(|model| {
                vec![ModelInfo {
                    id: model.clone(),
                    display_name: model.clone(),
                    capabilities: self.config.kind.default_capabilities(),
                }]
            })
            .unwrap_or_default())
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<TokenUsage, ProviderError> {
        let key = self
            .config
            .api_key
            .as_ref()
            .ok_or_else(|| ProviderError::MissingKey(self.config.id.clone()))?;

        let mut system = request.system.unwrap_or_default();
        let mut messages = Vec::new();
        for message in &request.messages {
            if message.role == Role::System {
                if !system.is_empty() {
                    system.push_str("\n\n");
                }
                system.push_str(&message.content);
            } else {
                messages.push(json!({
                    "role": message.role.as_anthropic(),
                    "content": message.content,
                }));
            }
        }

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens.unwrap_or(4_096),
            "messages": messages,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" },
            }]);
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }

        let url = format!("{}/v1/messages", self.config.base_url());
        let request_builder = self
            .client
            .post(url)
            .header("x-api-key", key.expose_secret())
            .header("anthropic-version", "2023-06-01")
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
            for stream_event in parse_event(&event.event, &value) {
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
            context_window: 200_000,
            max_output_tokens: 8_192,
            ..ModelCapabilities::default()
        })
    }
}

/// Parse one Anthropic SSE event.
pub fn parse_event(name: &str, value: &Value) -> Vec<StreamEvent> {
    let mut events = Vec::new();
    match name {
        "content_block_delta" => {
            let delta_type = value.pointer("/delta/type").and_then(Value::as_str);
            match delta_type {
                Some("text_delta") => {
                    if let Some(text) = value.pointer("/delta/text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(StreamEvent::TextDelta(text.to_string()));
                        }
                    }
                }
                Some("thinking_delta") => {
                    if let Some(text) = value.pointer("/delta/thinking").and_then(Value::as_str) {
                        if !text.is_empty() {
                            events.push(StreamEvent::ReasoningDelta(text.to_string()));
                        }
                    }
                }
                _ => {}
            }
        }
        "message_start" => {
            if let Some(usage) = value.pointer("/message/usage") {
                events.push(StreamEvent::UsageUpdate(TokenUsage {
                    prompt_tokens: usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as u32,
                    ..Default::default()
                }));
            }
        }
        "message_delta" => {
            if let Some(usage) = value.get("usage") {
                let completion_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32;
                events.push(StreamEvent::UsageUpdate(TokenUsage {
                    completion_tokens,
                    total_tokens: completion_tokens,
                    ..Default::default()
                }));
            }
        }
        _ => {}
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_thinking_deltas() {
        let text = json!({"delta": {"type": "text_delta", "text": "hi"}});
        assert!(matches!(
            parse_event("content_block_delta", &text).first(),
            Some(StreamEvent::TextDelta(value)) if value == "hi"
        ));
        let thinking = json!({"delta": {"type": "thinking_delta", "thinking": "hmm"}});
        assert!(matches!(
            parse_event("content_block_delta", &thinking).first(),
            Some(StreamEvent::ReasoningDelta(value)) if value == "hmm"
        ));
    }

    #[test]
    fn parses_usage_from_message_events() {
        let start = json!({"message": {"usage": {"input_tokens": 20}}});
        let delta = json!({"usage": {"output_tokens": 5}});
        let start_events = parse_event("message_start", &start);
        let delta_events = parse_event("message_delta", &delta);
        assert_eq!(
            start_events.first(),
            Some(&StreamEvent::UsageUpdate(TokenUsage {
                prompt_tokens: 20,
                ..Default::default()
            }))
        );
        assert_eq!(
            delta_events.first(),
            Some(&StreamEvent::UsageUpdate(TokenUsage {
                completion_tokens: 5,
                total_tokens: 5,
                ..Default::default()
            }))
        );
    }
}
