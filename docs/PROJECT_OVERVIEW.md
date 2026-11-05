# Project Overview: TradinationalTUI (`ttui`)

## 1. Vision and Objective

**TradinationalTUI** (binary name: `ttui`) is a lightweight, cross-platform, native Terminal User Interface (TUI) Integrated Development Environment (IDE) written in Rust. Its goal is to deliver the cohesive developer experience of a traditional graphical IDE (such as VS Code or Zed) directly inside any modern terminal emulator—without Electron, browser engines, or external GUI windows.

`ttui` unites the speed, low memory footprint, and modal ergonomics of terminal editors (Helix, Neovim) with the discoverable multi-panel layout, integrated terminal, project-wide search, Language Server Protocol (LSP) intelligence, and first-class AI coding agent workflows (Agent Client Protocol + Bring Your Own Key) of modern IDEs.

---

## 2. Core Design Principles

1. **Terminal-Native & Zero-GUI**: 100% of rendering occurs inside the host terminal emulator using ANSI/VT escape sequences over alternate screen buffers.
2. **Performance as a Hard Constraint**:
   - Cold startup to interactive frame in **< 25 ms** (target: 10–18 ms).
   - Idle Resident Set Size (RSS) in **< 25 MB** for a standard workspace.
   - **0% CPU** usage when idle (strictly event-driven loop; no busy-polling).
   - Input-to-screen latency **< 5 ms** (p99 < 8 ms) for typing and cursor movement.
3. **First-Class Protocol Standardization**:
   - **LSP (Language Server Protocol 3.17)** for language intelligence, diagnostics, code actions, symbols, and formatting.
   - **ACP (Agent Client Protocol v1 stable / v2 draft)** as the primary standardized JSON-RPC 2.0 interface for external AI coding agents (`opencode acp`, `command-code-acp`, `cursor-agent acp`, `claude-agent-acp`, `codex-acp`, `gemini --acp`).
   - **BYOK (Bring Your Own Key)** provider-agnostic LLM layer for direct inline completions, quick explanations, and chat without requiring an external agent daemon.
4. **Graceful Degradation (Zero-AI Usability)**:
   - If no ACP agent is installed and no BYOK API keys are configured, `ttui` operates as a pure, zero-overhead native code editor with zero background AI threads or network activity.
5. **Cross-Platform Parity**:
   - First-class support on **Linux**, **macOS**, and **Windows** (Windows Terminal, PowerShell, CMD, WSL) using unified platform abstractions for PTYs, file watching, credential storage, and path normalization.
6. **Modular, Testable Architecture**:
   - 16 strictly bounded modules communicating through typed message channels and explicit traits, enabling deterministic headless testing of the editor, UI layout, LSP client, and ACP state machines.

---

## 3. Target User Experience

`ttui` provides a familiar five-zone IDE workbench controllable via both **keyboard-first modal/chorded bindings** (Vim/Helix modal editing + VS Code-style leader/Ctrl shortcuts) and **optional mouse interactions** (click-to-focus, tab switching, split drag-resizing, scroll wheel):

```
+-----------------------------------------------------------------------------------+
| [Explorer] | [main.rs *] [lib.rs] [acp.rs]                      | [AI Assistant]  |
|            +----------------------------------------------------+                 |
| > src/     |  1 | use std::sync::Arc;                           | Session: Active |
|   main.rs  |  2 |                                               | Agent: OpenCode |
|   lib.rs   |  3 | pub fn main() -> anyhow::Result<()> {         |                 |
| > docs/    |  4 |     let app = App::new()?;                    | > Refactor LSP  |
|   Cargo.to |  5 |     app.run()                                 |   transport...  |
|            |  6 | }                                             |                 |
|            +----------------------------------------------------+ [Approve] [Deny]|
|            | [Terminal] [Problems (2)] [Output] [ACP Logs]      |                 |
|            | $ cargo test --workspace                           |                 |
|            | test result: ok. 42 passed; 0 failed               |                 |
+-----------------------------------------------------------------------------------+
| NORMAL | main* (+2 ~1) | rust-analyzer: Ready | E:0 W:2 | UTF-8 | LF | Ln 4, Col 9|
+-----------------------------------------------------------------------------------+
```

### Key Workbench Components
- **Left Sidebar (`explorer`)**: File tree with Git status badges, file creation/renaming/deletion, and workspace symbol outline.
- **Center Workbench (`editor`)**: Multi-tab, split-pane code editor with Ropey UTF-8 buffers, Tree-sitter syntax highlighting, multi-cursor editing, code folding, inline diagnostics, and Git diff gutters.
- **Bottom Panel (`terminal`, `lsp`, `output`)**: Multiplexed PTY terminal emulator (`portable-pty` + `vt100`), workspace problems/diagnostics table, LSP/ACP protocol trace logs, and task runner output.
- **Right Sidebar (`ai`, `acp`)**: Conversational AI assistant, ACP agent tool-call timeline, live diff previews, and interactive permission approval prompts (`session/request_permission`).
- **Overlays (`commands`, `search`)**:
  - **Command Palette (`Ctrl+Shift+P` / `:`)**: Fuzzy-searched registry of every IDE action, setting, theme, and agent command.
  - **File Finder (`Ctrl+P` / `<Space>f`)**: Lock-free parallel fuzzy file picker powered by `nucleo`.
  - **Global Search & Replace (`Ctrl+Shift+F`)**: Streaming ripgrep-powered regex search and project-wide replacement with live preview.
- **Status Bar**: Mode indicator (`NORMAL`, `INSERT`, `VISUAL`, `COMMAND`), Git branch and diff summary, active LSP server status, diagnostic counts, active ACP agent status, file encoding/line endings, and cursor coordinates.

---

## 4. Scope Boundaries & Non-Goals

| In Scope | Out of Scope (Non-Goals) |
|----------|--------------------------|
| Terminal-rendered TUI workbench (ANSI/Crossterm/Ratatui) | Custom GPU/windowed GUI frontend (Electron, Tauri, winit) |
| Native UTF-8 rope editing engine with Tree-sitter & multi-cursor | Embedded web browser or rich HTML/CSS preview panes |
| Full LSP 3.17 client (completion, hover, goto, rename, diagnostics) | Proprietary VS Code `.vsix` Node.js extension host runtime |
| Standardized ACP v1/v2 client & subprocess manager | Hardcoding vendor-specific closed agent protocols in core |
| Multi-provider BYOK streaming client (OpenAI, Anthropic, Gemini, Ollama) | Mandatory cloud accounts, telemetry, or forced AI features |
| Cross-platform Linux, macOS, and Windows Terminal support | Legacy 1970s dumb terminals lacking 256-color/TrueColor or VT sequences |

---

## 5. Documentation Map

All engineering specifications are maintained in `docs/`:

- [`TECH_STACK.md`](./TECH_STACK.md) — Comparative evaluation of languages, TUI frameworks, text engines, and crates.
- [`DECISIONS.md`](./DECISIONS.md) — Formal Architecture Decision Records (ADRs 001–010).
- [`ARCHITECTURE.md`](./ARCHITECTURE.md) — 16-module system architecture, event loop, concurrency model, and data flow.
- [`UI_DESIGN.md`](./UI_DESIGN.md) — Workbench layout engine, focus navigation, keybindings, themes, and incremental rendering.
- [`EDITOR_ENGINE.md`](./EDITOR_ENGINE.md) — Rope buffer architecture, multi-cursor engine, Tree-sitter integration, folding, and undo tree.
- [`ACP_INTEGRATION.md`](./ACP_INTEGRATION.md) — Agent Client Protocol (ACP) specification, lifecycle, permissions, and agent compatibility matrix.
- [`AI_PROVIDER_ARCHITECTURE.md`](./AI_PROVIDER_ARCHITECTURE.md) — BYOK provider traits, SSE streaming, OS keyring storage, and context assembly.
- [`LSP_INTEGRATION.md`](./LSP_INTEGRATION.md) — Language Server Protocol lifecycle, document sync, and intelligence routing.
- [`TERMINAL_INTEGRATION.md`](./TERMINAL_INTEGRATION.md) — Cross-platform PTY management (`portable-pty`), VT state machine, and shell integration.
- [`CROSS_PLATFORM.md`](./CROSS_PLATFORM.md) — Linux, macOS, and Windows abstractions, path handling, ConPTY, and terminal quirks.
- [`PERFORMANCE.md`](./PERFORMANCE.md) — Latency/memory budgets, dirty-region rendering, zero-allocation paths, and Criterion benchmarks.
- [`SECURITY.md`](./SECURITY.md) — Threat model, ACP permission sandboxing, credential storage, and workspace trust.
- [`TESTING.md`](./TESTING.md) — Unit, property-based, snapshot (`TestBackend`), mock LSP/ACP, and CI verification strategy.
- [`BUILD_AND_INSTALL.md`](./BUILD_AND_INSTALL.md) — Reproducible environment setup and build instructions for Linux, macOS, and Windows.
- [`ROADMAP.md`](./ROADMAP.md) — Phased implementation plan (Phases 0 through 6) with acceptance criteria.
