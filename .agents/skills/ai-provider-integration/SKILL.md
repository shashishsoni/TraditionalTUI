---
name: ai-provider-integration
description: Build and extend Bring Your Own Key (BYOK) LLM providers (OpenAI-compatible, Anthropic, Google Gemini, Ollama, llama.cpp), SSE streaming parsers, OS keyring credential storage, and context token budgeting in src/providers/ and src/ai/.
---

# AI Provider Integration Skill (`ai-provider-integration`)

## 1. Purpose
Implement provider-agnostic, streaming BYOK LLM integrations in `src/providers/` and context orchestration in `src/ai/`, completely decoupled from external ACP agents (`src/acp/`).

## 2. Relevant Architecture
- **`AiProvider` Trait (`src/providers/mod.rs`)**: Unified async interface for model discovery (`list_models`), capability probing (`probe_capabilities`), and token-streamed completion (`stream_completion`).
- **Wire Adapters**:
  - `src/providers/openai.rs`: OpenAI, OpenRouter, DeepSeek, Groq, vLLM, and local `llama.cpp` (`llama-server`).
  - `src/providers/anthropic.rs`: Anthropic `/v1/messages` with SSE streaming and ephemeral prompt caching.
  - `src/providers/gemini.rs`: Google Gemini `streamGenerateContent?alt=sse` with header-based auth (`x-goog-api-key`).
  - `src/providers/ollama.rs`: Local Ollama daemon (`/api/tags`, `/api/show`, `/api/chat`, `/api/generate` FIM).
- **`CredentialStore` (`src/providers/credentials.rs`)**: OS keyring integration (`keyring` crate) + environment variable fallback + `secrecy::SecretString`.

## 3. Implementation Guidelines
1. **Zero Plaintext Secrets**:
   - Store and retrieve API keys via `CredentialStore` (OS keyring service `"ttui-ide"` or env var fallback).
   - Wrap keys in `secrecy::SecretString` and only call `.expose_secret()` when constructing the outbound HTTP request header.
   - Never pass API keys in URL query parameters (use `x-goog-api-key` header for Gemini, `x-api-key` for Anthropic, `Authorization: Bearer` for OpenAI).
2. **Responsive SSE Streaming & Cancellation**:
   - Parse Server-Sent Events via `eventsource-stream` over `reqwest::Response::bytes_stream()`.
   - Select on `cancel_token.cancelled()` alongside stream reads so pressing `Esc` or typing in the editor aborts the HTTP stream immediately.
3. **Graceful Zero-AI Mode**:
   - If no provider is configured or no API key is present, `AiCoordinator` must remain idle with zero background timers, zero error popups on startup, and zero network requests.
4. **TLS & Loopback Safety**:
   - Enforce `https://` for remote endpoints; permit `http://` only for loopback addresses (`127.0.0.1`, `localhost`, `[::1]`) used by Ollama and `llama.cpp`.

## 4. Code Conventions
- Normalize provider-specific HTTP errors (401 Unauthorized, 429 Rate Limit, 5xx Server Error) into structured `ProviderError` variants with actionable user remediation messages.
- Track `TokenUsage` (`prompt_tokens`, `completion_tokens`, `cached_tokens`) on every stream completion and emit `AppEvent::AiUsageUpdated`.

## 5. Dependencies
- `reqwest` (`rustls-tls`, `stream`, `json`)
- `eventsource-stream`
- `keyring`
- `secrecy`
- `tokio-util` (`sync::CancellationToken`)
- `serde`, `serde_json`

## 6. Common Mistakes to Avoid
- **Mistake**: Coupling BYOK HTTP chat logic with ACP JSON-RPC session state.
  - **Fix**: Keep `src/providers/` strictly for stateless HTTPS/SSE LLM APIs and `src/acp/` for stateful stdio coding agents.
- **Mistake**: Logging full HTTP request headers in `tracing::debug!`.
  - **Fix**: Redact `Authorization`, `x-api-key`, and `x-goog-api-key` headers.
- **Mistake**: Sending an entire 10,000-line file on every inline completion keystroke.
  - **Fix**: Use `ContextAssembler` to bound prefix/suffix windows and respect the model's `context_window`.

## 7. Testing Requirements
- Unit-test SSE chunk parsing for OpenAI, Anthropic, Gemini, and Ollama using recorded multi-chunk stream fixtures (including `[DONE]`, usage deltas, error frames, and immediate cancellation).

## 8. Project Documentation References
- [`docs/AI_PROVIDER_ARCHITECTURE.md`](../../../docs/AI_PROVIDER_ARCHITECTURE.md)
- [`docs/SECURITY.md`](../../../docs/SECURITY.md)
