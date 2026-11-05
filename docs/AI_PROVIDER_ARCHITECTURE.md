# Bring Your Own Key (BYOK) AI Provider Architecture (`AI_PROVIDER_ARCHITECTURE.md`)

## 1. Architectural Separation: BYOK Providers vs. ACP Agents

`TradinationalTUI (ttui)` keeps **ACP Agents (`src/acp/`)** and **BYOK Providers (`src/providers/`)** in two completely decoupled architectural layers coordinated only at the UI presentation layer (`src/ai/`):

| Dimension | ACP Layer (`src/acp/`) | BYOK Provider Layer (`src/providers/`) |
|-----------|------------------------|----------------------------------------|
| **Protocol** | Stateful JSON-RPC 2.0 over stdio subprocess | Stateless HTTPS REST + Server-Sent Events (SSE) / NDJSON |
| **Execution Owner** | External agent controls loop, planning, and tools | `ttui` controls prompt construction, context window, and single-turn/chat streaming |
| **Primary Use Cases** | Autonomous multi-file refactoring, running tests, agentic workflows | Sub-second inline ghost-text completion, quick code explanation (`K` / `<Space>e`), single-buffer transforms, offline local LLM queries |
| **Configuration** | Agent binary command + args | Provider endpoint URL, model ID, OS keyring API key |

---

## 2. Provider-Independent Core Trait (`src/providers/mod.rs`)

All BYOK integrations implement the asynchronous `AiProvider` trait:

```rust
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Unique identifier ("openai", "anthropic", "gemini", "ollama", or custom slug)
    fn id(&self) -> &str;

    /// Human-readable display name
    fn display_name(&self) -> &str;

    /// Discover available models from the endpoint (or return configured static list)
    async fn list_models(&self, cancel: CancellationToken) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Stream chat/completion tokens and tool/diff proposals
    async fn stream_completion(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<TokenUsage, ProviderError>;

    /// Probe endpoint health and model capabilities (vision, FIM, tool-calling, context window)
    async fn probe_capabilities(&self, model: &str) -> Result<ModelCapabilities, ProviderError>;
}
```

### Normalized Domain Types

```rust
#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub supports_streaming: bool,
    pub supports_fim_completion: bool, // Fill-in-the-middle for inline ghost text
    pub supports_system_prompt: bool,
    pub supports_reasoning_effort: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    UsageUpdate(TokenUsage),
    Completed(StopReason),
}
```

---

## 3. Supported Providers & Wire Adapters

### 3.1 OpenAI-Compatible Adapter (`src/providers/openai.rs`)
- **Targets**: OpenAI (`https://api.openai.com/v1`), OpenRouter, DeepSeek, Groq, Together, Mistral, vLLM, **llama.cpp `llama-server` (`http://127.0.0.1:8080/v1`)**, and LM Studio.
- **Endpoints**: `GET /models`, `POST /chat/completions` (`stream: true`, `stream_options: { include_usage: true }`).
- **Authentication**: `Authorization: Bearer <SECRET_KEY>` (optional for local `llama.cpp` / `vLLM` servers).

### 3.2 Anthropic Adapter (`src/providers/anthropic.rs`)
- **Targets**: Anthropic Claude API (`https://api.anthropic.com/v1/messages`).
- **Headers**: `x-api-key: <SECRET_KEY>`, `anthropic-version: 2023-06-01`.
- **SSE Events Handled**: `message_start`, `content_block_start`, `content_block_delta` (`text_delta`, `thinking_delta`), `message_delta` (usage & stop reason), `message_stop`.
- **Prompt Caching**: Automatically attaches `cache_control: { "type": "ephemeral" }` to static system prompts and large repository context blocks to minimize latency and token cost.

### 3.3 Google Gemini Adapter (`src/providers/gemini.rs`)
- **Targets**: Google Generative Language API (`https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse`).
- **Authentication**: `x-goog-api-key: <SECRET_KEY>` header (never placed in URL query strings where it could leak into proxy or debug logs).
- **Capabilities**: Queries `GET /v1beta/models/{model}` to auto-detect `inputTokenLimit` (up to 1M–2M tokens) and `outputTokenLimit`.

### 3.4 Ollama Local Adapter (`src/providers/ollama.rs`)
- **Targets**: Local Ollama daemon (`http://127.0.0.1:11434`).
- **Endpoints**:
  - `GET /api/tags` to list locally pulled models (`qwen2.5-coder`, `deepseek-coder-v2`, `codellama`, etc.).
  - `POST /api/show` to inspect exact context length (`num_ctx`), quantization level, and template capabilities.
  - `POST /api/chat` and `POST /api/generate` (supporting fast Fill-In-the-Middle `prompt` + `suffix` inline code completions).

---

## 4. Secure API-Key Management (`src/providers/credentials.rs`)

API keys **MUST NEVER** be written in plaintext to `config.toml` or workspace files.

1. **Resolution Order**:
   - **OS Keyring (Primary)**: Stored and retrieved via the `keyring` crate under service name `"ttui-ide"` and account `"provider:<provider_id>"`:
     - **Linux**: Freedesktop Secret Service (`libsecret` / GNOME Keyring / KWallet).
     - **macOS**: macOS Keychain Services.
     - **Windows**: Windows Credential Manager (`wincred`).
   - **Environment Variables (CI / Headless Fallback)**: `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, or custom `env_var` name specified in `config.toml`.
2. **In-Memory Protection**:
   - Keys are held in `secrecy::SecretString`, which zeroes memory on drop (`zeroize`) and prints `[REDACTED]` in any `Debug` formatting.
3. **Interactive Key Entry**:
   - Running `:ai set-key <provider>` in the Command Palette opens a masked password input modal, stores the secret directly in the OS keyring, and immediately discards the input buffer.

---

## 5. Context Management & Token Budgeting (`src/ai/context.rs`)

When the user triggers an inline action or sends a chat message via a BYOK provider, `ContextAssembler` constructs a bounded prompt within the model's `context_window`:

1. **Priority Tier 1 (Always Included)**:
   - User instruction + active visual selection or cursor surrounding window ($\pm 50$ lines) with cursor marker.
   - Active line LSP diagnostics (`Error` / `Warning` messages).
2. **Priority Tier 2 (Explicit `@` Mentions)**:
   - Files, symbols, or git diffs explicitly referenced via `@file:src/main.rs`, `@diff`, or `@diagnostics`.
3. **Priority Tier 3 (Sliding Context)**:
   - Tree-sitter structural outline (function/type signatures only, bodies elided) of open editor tabs.
4. **Cancellation**:
   - Every in-flight HTTP stream holds a `tokio_util::sync::CancellationToken`. Pressing `Esc` in the AI panel or continuing to type during an inline completion immediately drops the HTTP stream and aborts socket reads.
