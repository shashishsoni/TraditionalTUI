# System Architecture (`ARCHITECTURE.md`)

## 1. High-Level Architectural Overview

`TradinationalTUI (ttui)` follows an **Event-Driven, Single-Owner State, Multi-Actor Background Architecture**.

- **Main UI & State Loop (`app` + `ui`)**: Runs on the foreground task, owning the authoritative `AppState` and driving terminal rendering only when state mutations mark the frame dirty.
- **Asynchronous Subsystem Actors (`lsp`, `acp`, `providers`, `terminal`, `search`, `git`, `workspace` watcher)**: Execute on the `tokio` multi-threaded runtime or dedicated CPU worker pools (`rayon` / `nucleo`), communicating with `app` exclusively through bounded, strongly-typed message channels (`AppEvent` / `SubsystemCommand`).

```
+-----------------------------------------------------------------------------------+
|                                Terminal Emulator                                  |
|             (Linux PTY / macOS Terminal & iTerm2 / Windows Terminal)              |
+-----------------------------------+-----------------------------------------------+
                                    | Crossterm EventStream (Keys, Mouse, Resize)
                                    | & Batched ANSI Diff Flush (DEC Mode 2026)
                                    v
+-----------------------------------------------------------------------------------+
|                        `app` (Application Core & Event Loop)                      |
|  +-----------------+  +-----------------+  +-----------------+  +--------------+  |
|  |   `commands`    |  |    `config`     |  |   `workspace`   |  |   `editor`   |  |
|  | Command Palette |  | TOML Settings,  |  | Project Root,   |  | Rope Buffers,|  |
|  | & Keymap Router |  | Keymaps, Themes |  | Open Documents  |  | Cursors, AST |  |
|  +-----------------+  +-----------------+  +-----------------+  +--------------+  |
|                                         |                                         |
|                                         v                                         |
|                        `ui` (Ratatui Workbench & Compositor)                      |
|    [Explorer]  |  [Multi-Tab Code Editor]  |  [Bottom Terminal/Diag]  |  [AI/ACP] |
+----+-----------+-------------+-------------+------------+-------------+------+----+
     |                         |                          |                    |
     | Typed Async Channels (`tokio::sync::mpsc::Sender<AppEvent>`)            |
     v                         v                          v                    v
+----------+             +-----------+              +------------+      +-----------+
| `search` |             |   `lsp`   |              | `terminal` |      |   `ai`    |
| `ignore` |             | JSON-RPC  |              | `portable- |      | Orchestr. |
| `nucleo` |             | Client    |              |  pty` +    |      +-----+-----+
| `notify` |             | Multiplex |              | `vt100`    |            |
+----------+             +-----------+              +------------+     +------+------+
|  `git`   |                                                           |             |
| `gix` +  |                                                           v             v
| `imara`  |                                                      +--------+   +-----------+
+----------+                                                      | `acp`  |   |`providers`|
     ^                                                            | Official|  | BYOK HTTP |
     |                                                            | ACP SDK|   | SSE +     |
     +------------------------------------------------------------+--------+   | Keyring   |
                                    |                                          +-----------+
                                    v
                    +-------------------------------+
                    |          `platform`           |
                    | OS Paths, Clipboard, Signals, |
                    | ConPTY/Unix PTY, Keyring      |
                    +-------------------------------+
```

---

## 2. The 16 Core Modules & Responsibilities

The codebase is organized into 16 modular subsystems under `src/` (and structured so heavy domains can be extracted into workspace crates without changing public traits):

| # | Module | Path | Primary Responsibility | Public Interface Boundary |
|---|--------|------|------------------------|---------------------------|
| 1 | **`app`** | `src/app/` | Application lifecycle, main `tokio::select!` event loop, central `AppState` coordinator, dirty-frame scheduling, and graceful shutdown. | `App`, `AppState`, `AppEvent`, `Action`, `FocusTarget` |
| 2 | **`ui`** | `src/ui/` | Ratatui layout calculation, theme styling, widget rendering (Explorer, Editor, Tabs, Status Bar, Bottom Panel, AI Sidebar, Modals), and frame diff flushing. | `UiRenderer`, `LayoutRegions`, `Theme`, `WidgetView` |
| 3 | **`editor`** | `src/editor/` | UTF-8 `ropey::Rope` text buffer, multi-cursor & selection engine, modal state machine (`Normal`/`Insert`/`Visual`), transaction Undo/Redo tree, Tree-sitter AST & folding, bracket matching. | `Document`, `BufferId`, `EditorView`, `CursorSet`, `EditTransaction`, `EditMode` |
| 4 | **`workspace`** | `src/workspace/` | Multi-buffer registry, tab groups & split panes, dirty-buffer tracking, atomic file saving, workspace session persistence, and root detection. | `Workspace`, `PaneLayout`, `TabManager`, `DocumentHandle` |
| 5 | **`explorer`** | `src/explorer/` | Lazy-loaded hierarchical file tree, `notify` filesystem watcher integration, Git status decorations, and inline file CRUD operations. | `FileExplorer`, `TreeNode`, `ExplorerAction` |
| 6 | **`terminal`** | `src/terminal/` | Integrated PTY multiplexer (`portable-pty`), `vt100` screen parser, scrollback buffer, shell integration, and ACP-managed terminal instances. | `TerminalManager`, `PtySession`, `TerminalId`, `TerminalEvent` |
| 7 | **`lsp`** | `src/lsp/` | Async LSP 3.17 client manager, per-language server lifecycle, `Content-Length` JSON-RPC transport, incremental document sync, diagnostics store, hover/completion/goto/rename handlers. | `LspManager`, `LspClient`, `LspRequest`, `LspNotification`, `DiagnosticStore` |
| 8 | **`git`** | `src/git/` | Repository discovery (`gix`), current branch/head status, file tree status badges, and in-memory buffer vs. index line diff computation (`imara-diff`). | `GitRepository`, `GitFileStatus`, `DiffHunk`, `GutterDiff` |
| 9 | **`search`** | `src/search/` | Lock-free fuzzy file/symbol matcher (`nucleo`), project-wide regex search (`ignore` + `grep-searcher`), and transactional multi-file search-and-replace. | `SearchEngine`, `FuzzyPicker`, `ProjectSearchQuery`, `SearchMatch` |
| 10 | **`commands`** | `src/commands/` | Static & dynamic command registry, keybinding chord resolver, and Command Palette fuzzy filter & execution dispatcher. | `CommandRegistry`, `CommandId`, `CommandSpec`, `Keymap` |
| 11 | **`plugins`** | `src/plugins/` | Capability-gated extension registry, custom TOML tasks/grammars, and JSON-RPC stdio / WASI extension host lifecycle. | `PluginManager`, `PluginManifest`, `PluginCapability`, `PluginHost` |
| 12 | **`ai`** | `src/ai/` | Unified AI UX orchestrator: context collector (active selection, open buffers, diagnostics, `@file` mentions), inline ghost-text completions, diff previewer, and conversation state. | `AiCoordinator`, `ContextBundle`, `ChatThread`, `DiffProposal` |
| 13 | **`acp`** | `src/acp/` | First-class Agent Client Protocol (ACP v1/v2) client (`agent-client-protocol` SDK), agent registry & discovery, subprocess supervisor, streaming `session/update` handler, and `session/request_permission` gatekeeper. | `AcpController`, `AcpAgentProfile`, `AcpSession`, `PermissionRequest`, `PermissionDecision` |
| 14 | **`providers`** | `src/providers/` | Bring Your Own Key (BYOK) LLM client implementations (OpenAI-compatible, Anthropic, Gemini, Ollama), SSE token streaming, token usage accounting, and OS keyring storage. | `AiProvider`, `ProviderConfig`, `CompletionRequest`, `StreamChunk`, `CredentialStore` |
| 15 | **`config`** | `src/config/` | Hierarchical TOML configuration loader (default -> global user config -> workspace `.ttui/config.toml`), live config reload, and validation. | `Config`, `EditorConfig`, `UiConfig`, `AiConfig`, `AcpConfig` |
| 16 | **`platform`** | `src/platform/` | OS-specific abstractions for Linux, macOS, and Windows: standard directories, path canonicalization, clipboard (`arboard`/OSC 52), process group termination, and shell detection. | `PlatformPaths`, `ClipboardProvider`, `ProcessKiller`, `ShellDetector` |

---

## 3. Dependency Rules & Layering (DAG)

To prevent circular dependencies and keep compilation fast, modules adhere to a strict Layered Directed Acyclic Graph (DAG):

1. **Layer 0 — Foundation (`platform`, `config`)**:
   - Depends only on standard library and foundational crates (`serde`, `toml`, `dirs`).
   - Zero knowledge of UI, Editor, LSP, or AI.
2. **Layer 1 — Domain Engines (`editor`, `git`, `search`, `terminal`, `providers`, `acp`, `lsp`)**:
   - Depend on Layer 0.
   - Completely independent of `ratatui` and `ui`. Every domain engine can be instantiated and tested in a headless CLI or unit test without a terminal!
3. **Layer 2 — Coordination & Workbench State (`workspace`, `explorer`, `commands`, `plugins`, `ai`)**:
   - Coordinate across Layer 1 domain engines.
   - Still independent of raw terminal rendering primitives.
4. **Layer 3 — Presentation & Runtime (`ui`, `app`)**:
   - `ui` reads immutable borrows of `AppState` (`&AppState`) and renders into `ratatui::Frame`.
   - `app` wires the main event loop, translating `crossterm` events and background `AppEvent`s into domain mutations.

> **Hard Architectural Rule**: Domain modules (`editor`, `lsp`, `acp`, `providers`, `search`, `git`, `terminal`) MUST NEVER import `ratatui` or `crossterm`.

---

## 4. Event Loop & Concurrency Model

`ttui` uses a single non-blocking `tokio::select!` loop in `src/app/mod.rs` with **render coalescing**:

```rust
pub async fn run(&mut self, terminal: &mut TuiTerminal) -> Result<()> {
    let mut term_events = crossterm::event::EventStream::new();
    let mut render_interval = tokio::time::interval(Duration::from_micros(8_333)); // 120 FPS cap
    render_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    self.dirty = true;

    while !self.state.should_quit {
        if self.dirty {
            terminal.draw(|frame| self.ui.render(frame, &self.state))?;
            self.dirty = false;
        }

        tokio::select! {
            biased;

            // 1. Highest priority: Direct user keyboard/mouse/resize input
            Some(Ok(raw_event)) = term_events.next() => {
                self.handle_terminal_event(raw_event)?;
                // Drain any immediately queued input events before redrawing
                while let Some( Some(Ok(next_ev)) ) = term_events.next().now_or_never() {
                    self.handle_terminal_event(next_ev)?;
                }
                self.dirty = true;
            }

            // 2. Background subsystem events (LSP, ACP stream, BYOK tokens, PTY output, FS watch)
            Some(app_event) = self.event_rx.recv() => {
                self.handle_app_event(app_event)?;
                // Coalesce bursty events (e.g., 50 PTY lines or AI token chunks) up to 64 per tick
                for _ in 0..64 {
                    match self.event_rx.try_recv() {
                        Ok(ev) => self.handle_app_event(ev)?,
                        Err(_) => break,
                    }
                }
                self.dirty = true;
            }

            // 3. Frame tick only used when active animations/cursor blink or throttled streams exist
            _ = render_interval.tick(), if self.has_pending_visual_tick() => {
                self.dirty = true;
            }
        }
    }
    Ok(())
}
```

### Why This Loop Achieves 0% Idle CPU and < 5 ms Input Latency
1. **`biased` Priority**: User input (`term_events`) always preempts background LSP/ACP messages.
2. **Event Coalescing**: When a compiler dumps 10,000 lines into the integrated PTY or an AI agent streams fast tokens, up to 64 messages are drained in microseconds before a single terminal frame diff is computed.
3. **Conditional Tick**: The frame timer branch is guarded by `if self.has_pending_visual_tick()`. When no stream is active and the user is not typing, `tokio::select!` parks the thread completely on OS epoll/kqueue/IOCP—resulting in **0.0% idle CPU usage**.

---

## 5. Error Handling & Panic Safety Strategy

In a terminal IDE, an unhandled panic that leaves the terminal in Raw Mode with Mouse Capture enabled breaks the user's shell session.

1. **Terminal Guard RAII (`TerminalSession`)**:
   - Entering alternate screen, raw mode, bracketed paste, and mouse capture is encapsulated in a `TerminalSession` struct whose `Drop` implementation unconditionally restores the terminal state.
2. **Panic Hook Installation**:
   - Before entering raw mode, `app` installs a custom `std::panic::set_hook` that immediately runs terminal restoration sequences (`disable_raw_mode`, `LeaveAlternateScreen`, `DisableMouseCapture`, `DisableBracketedPaste`, cursor show) before printing the structured backtrace and crash log path.
3. **Typed Error Boundaries**:
   - Internal modules define domain-specific error enums via `thiserror` (`EditorError`, `LspError`, `AcpError`, `ProviderError`, `WorkspaceError`).
   - Subsystem errors (e.g., an LSP server crashing or an ACP agent returning a JSON-RPC error) are caught, logged via `tracing`, and surfaced as non-fatal status bar notifications or panel banners—never crashing the editor.
