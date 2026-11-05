# TradinationalTUI (`ttui`)

**TradinationalTUI (`ttui`)** is a lightweight, extremely fast, cross-platform Terminal User Interface (TUI) IDE written in Rust. It recreates the essential multi-panel workbench experience of VS Code directly inside your terminal emulator—with native performance, first-class **Agent Client Protocol (ACP)** support, and flexible **Bring Your Own Key (BYOK)** AI provider integration.

---

## Highlights

- **100% Terminal-Native**: Runs inside any modern terminal emulator on **Linux**, **macOS** (`Terminal.app`, `iTerm2`, Ghostty, WezTerm), and **Windows** (`Windows Terminal`, PowerShell, CMD). Zero Electron, browser, or GUI dependencies.
- **VS Code-Inspired 5-Zone Workbench**:
  - **Left Activity Bar**: Explorer, Source Control (branch and changed files), and project-wide Search views.
  - **Center Workbench**: Multi-tab code editor powered by `ropey::Rope` UTF-8 B-trees, multi-cursor editing, Vim/Helix modal & VS Code keymap presets, horizontal scrolling, and incremental syntax highlighting.
  - **Bottom Panel**: Multiplexed PTY terminal (`portable-pty` + `vt100`), workspace `Problems` (diagnostics), `LSP` logs, and `ACP Trace` inspector.
  - **Right Sidebar**: Optional AI Assistant & ACP Coding Agent timeline with inline permission cards and red/green edit diffs.
  - **Command Palette & Fuzzy Pickers**: Instant command execution (`Ctrl+Space` / `:`), lock-free fuzzy file navigation (`Ctrl+P` via `nucleo`), and streaming ripgrep project search (`Alt+F`).
- **First-Class Agent Client Protocol (ACP)**:
  - Built against the official `agent-client-protocol` specification over stdio JSON-RPC 2.0.
  - Supports native ACP agents (`opencode acp`, `command-code-acp`, `cursor-agent acp`, `gemini --acp`, `goose acp`) and official ACP adapters (`claude-agent-acp`, `codex-acp`).
  - Interactive permission gatekeeper (`session/request_permission`) and workspace-confined `fs/*` and `terminal/*` execution.
- **Decoupled BYOK AI Providers**:
  - Connect your own API keys (stored securely in the OS Keyring) or local servers for OpenAI-compatible APIs, Anthropic Claude, Google Gemini, Ollama, and `llama.cpp`.
  - **Zero-AI Ready**: Remains 100% functional with zero background overhead when no AI provider or ACP agent is configured.

---

## Documentation & Architecture Blueprint

Before modifying or extending `ttui`, read [`AGENTS.md`](./AGENTS.md) and the full specification suite under [`docs/`](./docs/):

- [`docs/PROJECT_OVERVIEW.md`](./docs/PROJECT_OVERVIEW.md) — Vision, UX layout, and core design goals.
- [`docs/TECH_STACK.md`](./docs/TECH_STACK.md) & [`docs/DECISIONS.md`](./docs/DECISIONS.md) — Technology evaluation & ADRs (001–010).
- [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) — 16-module architecture, event loop, and concurrency model.
- [`docs/UI_DESIGN.md`](./docs/UI_DESIGN.md) — Workbench layout, focus management, themes, and DEC Mode 2026 rendering.
- [`docs/EDITOR_ENGINE.md`](./docs/EDITOR_ENGINE.md) — Rope buffer, multi-cursor engine, Undo/Redo DAG, and Tree-sitter AST pipeline.
- [`docs/ACP_INTEGRATION.md`](./docs/ACP_INTEGRATION.md) — Official ACP lifecycle, native vs. adapter matrix, and permission gatekeeper.
- [`docs/AI_PROVIDER_ARCHITECTURE.md`](./docs/AI_PROVIDER_ARCHITECTURE.md) — BYOK provider traits, SSE streaming, and OS keyring credentials.
- [`docs/LSP_INTEGRATION.md`](./docs/LSP_INTEGRATION.md) — Multiplexed LSP 3.17 client and UTF-8/UTF-16 position translation.
- [`docs/TERMINAL_INTEGRATION.md`](./docs/TERMINAL_INTEGRATION.md) — Cross-platform PTY (`portable-pty`) and `vt100` emulation.
- [`docs/CROSS_PLATFORM.md`](./docs/CROSS_PLATFORM.md) — Linux, macOS, and Windows platform rules and terminal quirks.
- [`docs/PERFORMANCE.md`](./docs/PERFORMANCE.md) — Quantitative latency/memory budgets and Criterion benchmarks.
- [`docs/SECURITY.md`](./docs/SECURITY.md) — Workspace trust, ACP path confinement, and secret zeroization.
- [`docs/TESTING.md`](./docs/TESTING.md) — Unit, property-based (`proptest`), and headless `TestBackend` snapshot testing.
- [`docs/BUILD_AND_INSTALL.md`](./docs/BUILD_AND_INSTALL.md) — Reproducible installation commands for Linux, macOS, and Windows.
- [`docs/ROADMAP.md`](./docs/ROADMAP.md) — Phased implementation plan (Phases 0–6).

Reusable AI coding skills are located in [`.agents/skills/`](./.agents/skills/).

---

## Quick Start

```bash
# Verify environment and health check
cargo run -- --check-health

# Launch TradinationalTUI in the current directory
cargo run -- .

# Run full test suite, linter, and benchmark verification
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo bench --no-run
```

### First run

`ttui` opens in **INSERT** mode: type immediately, press `Esc` for normal-mode motions.

| Shortcut | Action |
|----------|--------|
| `Ctrl+Space` | Command palette |
| `Ctrl+P` | Fuzzy file finder |
| `Ctrl+T` | Open integrated terminal (`Ctrl+Space` returns to panel navigation) |
| `Ctrl+G` | Connect an installed ACP agent (Command Code, OpenCode, Gemini, …) |
| `Ctrl+O` | Create/open `~/.config/ttui/config.toml` |
| `Alt+O` | Open another project folder |
| `Alt+B` | Switch Git branch (or click the branch in the status bar) |
| `Alt+E` / `Alt+G` / `Alt+F` | Explorer / Source Control / Search sidebar |
| `F5` / `F6` / `F7` | Explorer / Source Control / Search sidebar |
| `Alt+D` / `F8` | Review the current AI edit diff |
| `Ctrl+Q` | Quit (twice when buffers are dirty) |
| `F1` | Keymap help |

The left activity rail switches between Explorer, Source Control and Search. Source Control
lists the current branch and every changed file; Search runs in the sidebar with clickable
results. ACP edit proposals appear inline in the AI sidebar with red removed and green added
lines; `Alt+D` opens the full diff.

Click any file in the explorer to open it, click the bottom tabs to switch between
Terminal, Problems, Output and ACP Logs, and click the branch name in the status bar to
open the branch switcher. The branch switcher also contains **Open Folder…** for loading
any directory as a new project without restarting.

`Ctrl+O` writes a commented starter config on first use. Configure the theme, terminal
shell, BYOK providers, LSP servers and ACP agents there. Workspace-local
`.ttui/config.toml` overrides the global file after `:trust`.
