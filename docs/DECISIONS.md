# Architecture Decision Records (ADRs)

This document captures the foundational Architecture Decision Records (ADRs) governing **TradinationalTUI (`ttui`)**.

---

## ADR-001: Select Rust (Edition 2024) as Primary Implementation Language

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  `ttui` must run natively across Linux, macOS, and Windows with < 25 ms startup time, < 25 MB idle RAM, < 5 ms keystroke-to-render latency, and high concurrency across file indexing, incremental AST parsing, LSP servers, PTY shells, and ACP coding agents.
- **Options Considered**:
  1. **Rust**: Zero-cost abstractions, no GC pauses, `Send`/`Sync` thread safety, official `agent-client-protocol` SDK, `ratatui`, `ropey`, `tree-sitter`, `ignore`, `nucleo`, `portable-pty`.
  2. **Go**: Fast compilation and `bubbletea` ecosystem, but GC heap overhead, expensive CGo FFI calls for `tree-sitter`, and no official ACP Rust-equivalent SDK.
  3. **Zig**: Minimal binaries and C ABI integration (`libvaxis`), but pre-1.0 stdlib instability and lack of native ACP/LSP/keyring crates.
  4. **C / C++**: High performance, but memory-safety hazards across complex async state machines and fragmented Windows/Unix build toolchains.
- **Decision**: Use **Rust (Stable Toolchain, Edition 2024)**.
- **Consequences**:
  - Positive: Compile-time elimination of data races and use-after-free bugs; direct access to battle-tested crates from Helix, Zed, WezTerm, and ripgrep.
  - Negative: Longer cold compile times than Go, mitigated via modular Cargo workspace boundaries, `lld`/`mold` linkers, and `cargo-nextest`.

---

## ADR-002: Select Ratatui + Crossterm with Demand-Driven Synchronized Rendering

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  The IDE must render a complex multi-panel workbench (Explorer, Multi-Tab Editor, Bottom Terminal/Diagnostics, Right AI Panel, Overlays, Status Bar) without flicker, tearing, or idle CPU spin across Linux, macOS, and Windows Terminal.
- **Options Considered**:
  1. **`ratatui` + `crossterm`**: Cross-platform terminal control, cell-level double-buffered diffing, layout constraints, and headless `TestBackend` snapshot testing.
  2. **`termion`**: Unix-only; incompatible with Windows requirement.
  3. **`notcurses`**: Requires C library compilation and lacks ergonomic Windows native Console/ConPTY packaging.
- **Decision**: Use **`ratatui` + `crossterm` (`event-stream`)** driven by a dirty-flag, demand-driven render loop wrapped in **DEC Mode 2026 Synchronized Output (`\x1b[?2026h` ... `\x1b[?2026l`)**.
- **Consequences**:
  - Positive: 0% CPU usage when idle; zero tearing during rapid scrolling or streaming AI output; deterministic UI snapshot tests using `ratatui::backend::TestBackend`.
  - Negative: Immediate-mode widget construction requires discipline to avoid per-frame heap allocations (enforced via `Line`/`Span` borrowing from `RopeSlice`).

---

## ADR-003: Select Ropey UTF-8 B-Tree Rope for Text Buffer Engine

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  The editor must open 50–100 MB files instantaneously, support multi-cursor edits without $O(N)$ memory shifts, maintain Unicode grapheme correctness, and provide lock-free snapshots to background threads (Tree-sitter, LSP, search, AI context).
- **Options Considered**:
  1. **`ropey`**: UTF-8 B-tree rope with $O(\log N)$ edits, $O(\log N)$ byte/char/line conversions, and $O(1)$ copy-on-write cloning via `Arc`.
  2. **Gap Buffer**: Simple for single-cursor edits, but $O(N)$ penalty on distant multi-cursor edits and requires full buffer copy for background threads.
  3. **Piece Table**: Degrades in lookup complexity after thousands of fragmented edits unless periodically compacted.
- **Decision**: Use **`ropey::Rope`** paired with **`imara-diff`**, **`unicode-segmentation`**, and **`unicode-width`**, plus an immutable transaction-based Undo/Redo DAG.
- **Consequences**:
  - Positive: Instant $O(1)$ buffer snapshots for async workers; fast multi-cursor transactions; exact byte-offset alignment with Tree-sitter `InputEdit`.
  - Negative: Cursor and selection offsets must be normalized via helper methods rather than raw slice indexing.

---

## ADR-004: Dual-Tier Syntax Highlighting (Tree-sitter Primary + Syntect Fallback)

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  Syntax highlighting, code folding, bracket matching, and structural selection must remain under 1 ms per edit while also supporting hundreds of configuration and markup formats out of the box.
- **Decision**:
  - **Primary**: `tree-sitter` incremental AST parsing on a background worker pool, feeding viewport-scoped highlight queries, fold ranges, and indent guides.
  - **Fallback**: `syntect` line-by-line regex highlighting ONLY for visible viewport lines when no Tree-sitter grammar is registered for a file extension.
- **Consequences**:
  - Positive: Sub-millisecond structural updates on primary languages; broad filetype coverage without freezing on minified or huge files.
  - Negative: Must maintain a unified theme token mapping (`HighlightStyle`) shared between Tree-sitter capture names and TextMate scopes.

---

## ADR-005: Event-Driven Unidirectional Message Bus Architecture

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  With 16 distinct subsystems (UI, Editor, Workspace, Explorer, Terminal, LSP, Git, Search, Commands, Plugins, AI, ACP, Providers, Config, Platform), shared mutable state (`Arc<Mutex<AppState>>`) leads to lock contention, UI frame drops, and deadlocks.
- **Decision**:
  Adopt a **Single-Owner State + Async Event Bus** architecture:
  - The main `app` task exclusively owns mutable workbench & document state (`AppState`).
  - All background subsystems (`lsp`, `acp`, `providers`, `terminal`, `search`, `git`, `explorer` watcher) run on `tokio` tasks or worker threads and communicate via bounded `tokio::sync::mpsc` channels emitting typed `AppEvent` messages.
  - Outbound commands to subsystems use dedicated typed handle channels (`LspCommand`, `AcpCommand`, `PtyCommand`).
- **Consequences**:
  - Positive: Zero lock contention on the UI thread; deterministic state transitions; trivial unit/integration testing by feeding synthetic `AppEvent` sequences into `App::handle_event`.
  - Negative: Requires explicit message types for cross-module communication.

---

## ADR-006: First-Class Agent Client Protocol (ACP) via Official Rust SDK

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  External coding agents (OpenCode, Command Code, Cursor Agent CLI, Claude Agent, Codex, Gemini CLI) are converging on the **Agent Client Protocol (ACP)** over stdio JSON-RPC 2.0.
- **Decision**:
  Implement `src/acp/` using the official `agent-client-protocol` crate (v1 stable baseline + v2 draft readiness) as a first-class subsystem completely decoupled from the BYOK LLM provider layer:
  - Distinguish **Native ACP Agents** (`opencode acp`, `command-code-acp`, `cursor-agent acp`, `gemini --acp`) from **Adapter-Backed ACP Agents** (`claude-agent-acp`, `codex-acp`).
  - Route all agent file mutations (`fs/write_text_file`) and shell executions (`terminal/create`) through the IDE's **Permission & Diff Gatekeeper** (`session/request_permission`).
- **Consequences**:
  - Positive: Instant compatibility with 40+ ACP-compliant coding agents; full user control over file writes and shell commands inside the TUI.
  - Negative: Requires careful subprocess lifecycle management (graceful shutdown, zombie prevention, stderr ring-buffer capture).

---

## ADR-007: Decoupled BYOK Provider Layer with OS Keyring Storage

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  Users want direct LLM capabilities (inline completions, quick code explanations, lightweight chat, local Ollama/llama.cpp models) without needing an external ACP agent process, while keeping API keys out of plaintext config files.
- **Decision**:
  Create a standalone `providers` module defining an async `AiProvider` trait implemented for:
  1. **OpenAI-Compatible** (OpenAI, OpenRouter, DeepSeek, Groq, vLLM, llama.cpp server, LM Studio)
  2. **Anthropic Messages API** (`/v1/messages` with SSE streaming)
  3. **Google Gemini API** (`streamGenerateContent`)
  4. **Ollama Native / Local API**
  Store API keys exclusively in the OS credential store via `keyring` (with environment variable fallback `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`) wrapped in `secrecy::SecretString`.
- **Consequences**:
  - Positive: Zero plaintext secrets in `.ttui.toml` or `config.toml`; works offline with local Ollama/llama.cpp models; completely independent from ACP.

---

## ADR-008: Cross-Platform PTY via `portable-pty` and `vt100`

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  The bottom panel requires an embedded interactive shell supporting bash/zsh/fish on Linux/macOS and PowerShell/CMD/WSL on Windows, plus ACP agent-requested terminals (`terminal/create`).
- **Decision**:
  Use **`portable-pty`** (Unix `openpty`/`forkpty` + Windows `ConPTY`) paired with **`vt100::Parser`** for terminal state maintenance and cell rendering into the Ratatui bottom panel.
- **Consequences**:
  - Positive: Unified resize (` PtySize `), process group termination, and ANSI escape sequence emulation across Linux, macOS, and Windows 10/11.

---

## ADR-009: Out-of-Process / Sandboxed Plugin Architecture

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  Extensions must not crash the core editor, leak memory, or block the 60+ FPS render loop.
- **Decision**:
  Design the `plugins` module around a clean capability-based host API:
  - Phase 1–4: Declarative TOML keymaps, themes, language definitions, custom task runners, and external command/JSON-RPC stdio plugins.
  - Future expansion hook: Optional WASI (`wasmtime`) sandboxed guest plugins consuming the same host event/command RPC boundary.
- **Consequences**:
  - Positive: Core binary remains small and fast; misbehaving plugins cannot corrupt editor memory or block rendering.

---

## ADR-010: Strict Zero-Stdout Rule & Structured Diagnostic Ring Buffer

- **Status**: Accepted
- **Date**: 2026-11-05
- **Context**:
  While the terminal is in raw alternate-screen mode, any stray `println!`, `eprintln!`, or child process `stderr` write corrupts the terminal cell grid.
- **Decision**:
  - Forbid `println!`/`eprintln!`/`dbg!` in library/runtime code via Clippy lints (`clippy::print_stdout`, `clippy::print_stderr`, `clippy::dbg_macro`).
  - Route all `tracing` logs to a rotating log file in the OS state directory and an in-memory ring buffer viewable inside the IDE's **Output / Logs** bottom panel.
  - Always pipe `stdin`, `stdout`, and `stderr` of spawned LSP and ACP child processes (`Stdio::piped()`).
- **Consequences**:
  - Positive: Zero terminal corruption; instant in-editor debugging of LSP and ACP JSON-RPC traffic.
