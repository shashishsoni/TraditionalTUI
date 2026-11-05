# Phased Implementation Roadmap (`ROADMAP.md`)

This roadmap defines the incremental implementation phases for **TradinationalTUI (`ttui`)**, including deliverables, module dependencies, technical risks, and measurable acceptance criteria.

---

## Phase 0 — Research, Architecture & Blueprint (Completed in Current Task)
- **Deliverables**:
  - Comparative evaluation of languages, TUI engines, text ropes, syntax parsers, LSP, and ACP SDKs (`docs/TECH_STACK.md`, `docs/DECISIONS.md`).
  - Complete 16-document architecture specification in `docs/`.
  - 9 reusable AI implementation skills in `.agents/skills/`.
  - `AGENTS.md` repository constitution for coding agents.
- **Technical Risks Mitigated**:
  - Avoided Unix-only TUI libraries (`termion`) that break Windows compatibility.
  - Avoided gap buffers that stall on multi-cursor edits and async background tasks.
  - Selected official `agent-client-protocol` crate rather than inventing an ad-hoc agent protocol.

---

## Phase 1 — Environment, Foundation & Terminal Lifecycle (Current Foundation Scaffold)
- **Deliverables**:
  - Rust toolchain installation and verification.
  - Cargo project initialization with all 16 core module boundaries (`app`, `ui`, `editor`, `workspace`, `explorer`, `terminal`, `lsp`, `git`, `search`, `commands`, `plugins`, `ai`, `acp`, `providers`, `config`, `platform`).
  - Panic-safe `TerminalSession` RAII guard (raw mode, alternate screen, mouse capture, bracketed paste, DEC Mode 2026 synchronized output).
  - Non-blocking `tokio::select!` event loop with 0% idle CPU and demand-driven `dirty` rendering.
  - Initial Criterion benchmark harness (`benches/core_benchmarks.rs`) and headless `TestBackend` tests.
- **Acceptance Criteria**:
  - `cargo check`, `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` pass with 0 warnings.
  - `ttui --version` and `ttui --check-health` execute in < 15 ms.

---

## Phase 2 — TUI Workbench Foundation
- **Deliverables**:
  - Responsive 5-zone workbench layout (`Left Explorer`, `Center Multi-Tab Editor`, `Bottom Panel`, `Right AI Sidebar`, `Status Bar`) with auto-collapse for narrow terminals.
  - Focus navigation ring (`Ctrl+W h/j/k/l`, mouse click-to-focus, drag split resizing).
  - Interactive `FileExplorer` tree with lazy directory loading, file icons/badges, and CRUD operations.
  - Multi-tab bar, split panes (horizontal/vertical), and `CommandPalette` (`Ctrl+Shift+P` / `:`) + `FileFinder` (`Ctrl+P` via `nucleo`).
  - Built-in theme engine (`TokyoNight`, `CatppuccinMocha`, `GruvboxDark`, `OneDarkPro`, `CatppuccinLatte`) with 256-color fallback quantizer.
- **Acceptance Criteria**:
  - Full workbench renders in < 1.5 ms on a 200x60 grid in Criterion benchmarks.
  - Snapshot tests (`TestBackend`) verify all 5 panels and modal overlays.

---

## Phase 3 — Core Editing Experience
- **Deliverables**:
  - Complete `ropey::Rope` document editing with `SmallVec` multi-cursor engine (`CursorSet`) and Vim/Helix modal keybindings + VS Code preset.
  - Structural sharing Undo/Redo DAG (`UndoHistory`) with 350 ms typing coalescing.
  - Incremental Tree-sitter AST highlighting, code folding (`FoldMap`), bracket matching, and auto-indentation + `syntect` fallback.
  - Buffer-local regex search and interactive replace (`Ctrl+F` / `Ctrl+H` / `:%s`).
  - Atomic cross-platform file save with encoding (`UTF-8`, `UTF-16`) and line-ending (`LF`/`CRLF`) preservation.
- **Acceptance Criteria**:
  - `proptest` suite passes 10,000 random multi-cursor Unicode edit + undo/redo cycles.
  - 50 MB file loads in < 50 ms and scrolls at 120 FPS (< 2 ms/frame).

---

## Phase 4 — Developer Tooling (Terminal, Git, LSP, Search)
- **Deliverables**:
  - Embedded PTY terminal multiplexer (`portable-pty` + `vt100`) in Bottom Panel with resize synchronization and clickable compiler error paths.
  - Git integration (`gix` + `imara-diff`): branch badge, file explorer status markers, and live editor gutter diff markers (`+`, `~`, `_`).
  - Multiplexed LSP 3.17 client (`src/lsp/`): `didOpen`/`didChange`/`didSave`, diagnostics (`Problems` tab + inline virtual text), completion popup, hover docs, goto definition/references, rename, and formatting.
  - Streaming workspace-wide search and multi-file replace (`ignore` + `grep-searcher`).
- **Acceptance Criteria**:
  - Integration tests pass against a mock LSP server and real `rust-analyzer` smoke test.
  - Project-wide search across 50,000 files streams first results in < 15 ms.

---

## Phase 5 — AI Integration (First-Class ACP & BYOK Providers)
- **Deliverables**:
  - Official `agent-client-protocol` client (`src/acp/`) supporting `opencode acp`, `command-code-acp`, `cursor-agent acp`, `gemini --acp`, `claude-agent-acp`, and `codex-acp`.
  - Complete ACP lifecycle: discovery, subprocess supervision, `initialize`, `session/new`, `session/load`, streaming `session/update` (thoughts, markdown, tool calls, plans, diffs), and `session/cancel`.
  - Interactive `session/request_permission` gatekeeper + workspace-confined `fs/*` and `terminal/*` handlers.
  - Decoupled BYOK provider layer (`src/providers/`) for OpenAI-compatible, Anthropic, Gemini, and local Ollama/`llama.cpp` endpoints with OS `keyring` secret storage.
  - Right AI Sidebar UI + inline editor ghost-text completions and diff review splits.
- **Acceptance Criteria**:
  - Mock ACP agent integration tests verify full prompt turn, streaming tool calls, permission approval/rejection, and path traversal blocking.
  - IDE runs with zero overhead when AI/ACP is disabled or unconfigured.

---

## Phase 6 — Stabilization, Benchmarking & Cross-Platform Packaging
- **Deliverables**:
  - Cross-platform verification across Linux, macOS (`Terminal.app`, `iTerm2`), and Windows (`Windows Terminal`, PowerShell, CMD).
  - Full Criterion benchmark validation against all latency and memory budgets in `docs/PERFORMANCE.md`.
  - Security audit of workspace trust, ACP permission boundaries, and ANSI sanitization.
  - Release packaging (static/musl Linux tarballs, macOS universal binaries/Homebrew formula, Windows `.msi` / `winget` manifest, `cargo install`).
