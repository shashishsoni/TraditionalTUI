# Technology Stack Evaluation & Selection

This document records the technical research, quantitative trade-offs, and selection rationale for every layer of **TradinationalTUI (`ttui`)**.

---

## 1. Programming Language Evaluation

### Requirements
- Native machine-code compilation with no garbage-collection pauses during 60–120 FPS terminal rendering or rapid keystroke bursts.
- Memory safety and fearless concurrency for parallel fuzzy matching, incremental syntax parsing, LSP multiplexing, and ACP agent streams.
- First-class library ecosystem for TUI rendering, UTF-8 ropes, Tree-sitter, LSP types, PTY management, and ACP protocol SDKs.
- Cross-platform support across Linux (`x86_64`, `aarch64`), macOS (`aarch64`, `x86_64`), and Windows (`x86_64-pc-windows-msvc`).

### Comparative Matrix

| Criterion | Rust (2024 Edition) | Go (1.24+) | Zig (0.14+) | C++ (C++20/23) | C (C17) |
|-----------|---------------------|------------|-------------|----------------|---------|
| **Startup Time** | ~2–5 ms | ~8–15 ms (runtime init) | ~1–3 ms | ~2–5 ms | ~1–3 ms |
| **Idle Memory (RSS)** | **8–18 MB** | 25–45 MB (GC heap reserve) | 5–12 MB | 8–20 MB | 4–10 MB |
| **Memory & Thread Safety** | **Compile-time ownership/Send+Sync** | Memory-safe, data races possible | Manual allocator management | Manual / RAII (UB risks) | Manual (high UB risk) |
| **TUI Ecosystem** | **Ratatui, Crossterm, Termwiz** | Bubble Tea, Lipgloss, Tcell | libvaxis | FTXUI, Notcurses | ncurses, termbox2 |
| **Text Rope & Unicode** | **Ropey, Crop, unicode-segmentation** | Limited native rope libraries | Custom / C interop | Custom / PieceTable | Custom |
| **Tree-sitter Integration** | **Official `tree-sitter` crate** | CGo overhead on every FFI call | Native C ABI | Native C ABI | Native C ABI |
| **Official ACP SDK** | **Official `agent-client-protocol` crate** | Community/custom JSON-RPC | None (must build from scratch) | None | None |
| **LSP & Search Crates** | **`lsp-types`, `ignore`, `nucleo`, `gix`** | `gopls` internal packages | Manual implementation | `lspcpp` (fragmented) | Manual |

### Why Go, Zig, C, and C++ Were Rejected
1. **Go**: While Bubble Tea is ergonomic for simple TUIs, Go's garbage collector introduces heap overhead (25+ MB baseline when indexing large workspaces) and CGo incurs significant call overhead when invoking Tree-sitter C parsers thousands of times per frame. Furthermore, Go lacks battle-tested equivalents to `ropey`, `nucleo` (Helix's matcher), and the official `agent-client-protocol` SDK.
2. **Zig**: Zig offers exceptional binary size and C ABI interop (`libvaxis`, Flow editor), but its pre-1.0 standard library and async story remain in flux, and it lacks native equivalents for `agent-client-protocol`, `lsp-types`, OS keyring integration, and `gix`.
3. **C / C++**: FTXUI and Notcurses are fast, but manual memory management across highly concurrent LSP/ACP/PTY state machines invites use-after-free bugs and data races, while cross-platform Windows/Unix dependency management (CMake/vcpkg) is significantly more brittle than Cargo.

### Selected Language: **Rust (Stable Toolchain, 2024 Edition)**
Proven by **Helix**, **Zed**, **Lapce**, **WezTerm**, **Alacritty**, **ripgrep**, and **gitoxide**, Rust provides the exact intersection of deterministic zero-GC latency, compile-time concurrency safety, and the world's strongest terminal/editor crate ecosystem.

---

## 2. TUI Framework & Terminal Rendering Backend Evaluation

### Terminal Backends Compared

| Library | Platform Support | Event Model | Performance & Diffing | Verdict |
|---------|------------------|-------------|-----------------------|---------|
| **`crossterm`** | Linux, macOS, Windows (Console API + VT) | Sync & Async (`EventStream` via `futures`) | Zero-cost queued ANSI command batching, bracketed paste, Kitty keyboard protocol | **Selected** |
| **`termion`** | Unix only (Linux, macOS) | Sync iterators | Lightweight, no Windows support | Rejected (fails Windows requirement) |
| **`termwiz`** | Linux, macOS, Windows | buffered terminal surface | Used in WezTerm; heavier dependency tree | Secondary reference |
| **`notcurses`** | Unix + partial Windows via MSYS2 | C FFI | High-throughput multimedia blitting, complex C build | Rejected (C FFI build complexity on Windows) |

### TUI Frameworks Compared

| Framework | Architecture | Layout & Buffering | Ecosystem Maturity | Verdict |
|-----------|--------------|--------------------|--------------------|---------|
| **`ratatui` (0.29+)** | Immediate-mode double-buffered cell diffing | Constraint-based Cassowary/Flex layout, zero-flicker diffing, `TestBackend` for snapshot tests | De-facto standard in Rust | **Selected** |
| **Custom Helix-style Surface** | Retained/Immediate hybrid | Full control, requires writing all layout/table/border primitives from scratch | High maintenance burden | Borrow `Buffer` composition concepts inside `ratatui::widgets::StatefulWidget` |
| **`cursive`** | Retained-mode callback tree | Heavy abstraction, awkward for custom multi-cursor code buffers | Maintenance mode | Rejected |
| **`tui-realm`** | Elm-like subscriptions | Extra boilerplate wrapping Ratatui | Unnecessary indirection | Rejected |

### Selected TUI Stack: **`ratatui` + `crossterm`**
- **Double-Buffered Cell Diffing**: `ratatui` maintains `current` and `previous` `Buffer` grids in memory, computing only the mutated `(x, y)` cells per frame and emitting a single batched `write_all` flush wrapped in **Synchronized Output (`CSI ? 2026 h` ... `CSI ? 2026 l`)** to eliminate tearing.
- **Custom Editor Viewport Widget**: While sidebars, tabs, status bars, and modals use standard `ratatui` layouts, the central code editor and terminal viewport implement custom direct `ratatui::buffer::Buffer` cell writers for zero-allocation glyph placement.

---

## 3. Text-Editing Engine & Unicode Stack

### Data Structure Comparison

| Structure | Insert/Delete (Middle of 50MB file) | Line/Char Indexing | Memory Overhead | Concurrent Snapshots | Verdict |
|-----------|-------------------------------------|--------------------|-----------------|----------------------|---------|
| **UTF-8 B-Tree Rope (`ropey`)** | $O(\log N)$ (~sub-microsecond) | $O(\log N)$ native byte, char, and line-index | ~10–15% over raw bytes | **`Clone` is $O(1)$ via `Arc` B-tree nodes** | **Selected** |
| **Gap Buffer** | $O(N)$ on distant multi-cursor jumps | $O(N)$ unless external line table maintained | Low | Requires full copy ($O(N)$) for background tasks | Rejected (poor multi-cursor & async snapshot performance) |
| **Piece Table** | $O(P)$ (degrades after thousands of edits) | Requires auxiliary red-black tree | Low initially, grows with edit history | Good | Secondary alternative |
| **`xi-rope`** | $O(\log N)$ | Lacks built-in line/char conversion efficiency of Ropey | Moderate | Good, but crate is archived | Rejected |

### Why `ropey` is Ideal for a Terminal IDE
1. **$O(1)$ Copy-on-Write Snapshots**: Cloning a `ropey::Rope` only increments reference counts on root `Arc` nodes. Background threads (Tree-sitter parsing, LSP `didChange` formatting, ripgrep buffer search, AI context extraction) can read an immutable snapshot of a 100 MB file while the UI thread continues mutating the buffer at 120 FPS without locks.
2. **Multi-Cursor Native**: Applying $K$ simultaneous edits across a file takes $O(K \log N)$ time without shifting gigabytes of contiguous bytes (unlike a gap buffer).
3. **Unicode Correctness**: Combined with `unicode-segmentation` (UAX #29 grapheme clusters) and `unicode-width` (UAX #11 East Asian display widths), cursor movements never split multi-byte UTF-8 codepoints or emoji ZWJ sequences.

---

## 4. Syntax Highlighting & Structural Code Intelligence

| Technology | Role | Rationale |
|------------|------|-----------|
| **`tree-sitter` (0.24+)** | Primary incremental AST parser, syntax highlighting, code folding ranges, bracket matching, and structural selection | Incremental re-parsing in $< 1\text{ ms}$ on edits; error-tolerant ASTs; powers Helix, Neovim, and Zed. |
| **`syntect`** | Fallback regex/TextMate syntax highlighter for log files, `.env`, or niche config formats without compiled Tree-sitter grammars | Ensures broad out-of-the-box syntax highlighting coverage without blocking startup. |

---

## 5. Language Server Protocol (LSP) Stack

- **`lsp-types` (0.97+)**: Strongly typed Rust structs/enums for the complete LSP 3.17 specification (including inlay hints, semantic tokens, code actions, rename, and workspace diagnostics).
- **Custom Async Transport (`tokio::process` + `tokio-util::codec`)**: Lightweight `Content-Length: ...\r\n\r\n` JSON-RPC 2.0 framing codec supporting multiplexed language servers per workspace (e.g., `rust-analyzer` + `taplo` + `marksman` running concurrently).

---

## 6. Agent Client Protocol (ACP) & BYOK AI Stack

### Agent Client Protocol (ACP)
- **Official Crate**: **`agent-client-protocol` (v5+, `features = ["process", "stdio"]`)** maintained by the Agent Client Protocol organization / Zed Industries.
- **Protocol Coverage**:
  - **ACP v1 (Stable)**: `initialize`, `authenticate`, `session/new`, `session/load`, `session/prompt`, `session/set_mode`, `session/cancel`, `session/update`, `session/request_permission`, `fs/read_text_file`, `fs/write_text_file`, `terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`, `elicitation/create`.
  - **ACP v2 (Draft Ready)**: Modular adapter boundary supporting `session/resume`, `session/list`, `session/close`, `session/set_config_option`, and `auth/login`.

### Bring Your Own Key (BYOK) Provider Layer
- **HTTP & Streaming**: `reqwest` (`rustls-tls`, HTTP/2, connection pooling) + `eventsource-stream` for low-latency Server-Sent Events (SSE) token streaming and NDJSON parsing (for Ollama).
- **Credential Security**: `keyring` crate for native OS secret storage (Linux Secret Service/libsecret, macOS Keychain, Windows Credential Manager) + `secrecy::SecretString` (zeroizes keys on drop and redacts `Debug`/`Display` logs).

---

## 7. File System, Search, Git & Terminal Emulation

| Subsystem | Selected Crates | Rationale |
|-----------|-----------------|-----------|
| **Fuzzy Matcher** | `nucleo` / `nucleo-matcher` | Helix's lock-free, multi-threaded fuzzy matcher; orders of magnitude faster than single-threaded `fuzzy-matcher` on 500k+ file repos. |
| **File & Content Search** | `ignore` + `grep-regex` + `grep-searcher` | The exact engine powering `ripgrep`; respects `.gitignore`, `.ignore`, and hidden file rules natively. |
| **File Watcher** | `notify` + `notify-debouncer-mini` | Cross-platform event notification (`inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows). |
| **Integrated PTY** | `portable-pty` + `vt100` | WezTerm's battle-tested cross-platform PTY spawner (Unix openpty + Windows ConPTY) paired with an in-memory VT100/ANSI screen parser. |
| **Git Integration** | `gix` (gitoxide) + `imara-diff` | Pure-Rust, high-performance repository inspection (branch, status, HEAD tree) and histogram line diffing for live editor gutter markers (`+`, `~`, `_`). |
| **Configuration** | `serde` + `toml` + `dirs` | Human-readable TOML configuration with XDG / macOS Application Support / Windows AppData directory resolution. |
| **Logging & Telemetry** | `tracing` + `tracing-subscriber` + `tracing-appender` | Structured non-blocking file/panel logging (never writes to `stdout`/`stderr` while TUI is in raw mode). |

---

## 8. Final Stack Summary Table

| Layer | Technology / Crate | Version Policy |
|-------|--------------------|----------------|
| Language | Rust (Edition 2024) | Stable (`1.85+` / latest stable) |
| Async Runtime | `tokio` | `1.x` (`rt-multi-thread`, `process`, `io-util`, `sync`, `time`, `macros`) |
| TUI Renderer | `ratatui` + `crossterm` | `ratatui 0.29+`, `crossterm 0.28+` (`event-stream`) |
| Text Engine | `ropey` + `unicode-segmentation` + `unicode-width` | `ropey 1.6+` |
| Syntax & Folding | `tree-sitter` + `syntect` | `tree-sitter 0.24+`, `syntect 5.2+` |
| Fuzzy Engine | `nucleo-matcher` | `0.3+` |
| Search Engine | `ignore` + `grep-regex` + `grep-searcher` | `ignore 0.4+`, `grep-* 0.1+` |
| LSP Protocol | `lsp-types` + `serde_json` | `lsp-types 0.97+` |
| ACP Protocol | `agent-client-protocol` | `5.x` (`process`, `stdio`) |
| BYOK AI Client | `reqwest` + `eventsource-stream` + `keyring` + `secrecy` | `reqwest 0.12+` (`rustls-tls`) |
| Integrated Terminal | `portable-pty` + `vt100` | `portable-pty 0.8+`, `vt100 0.15+` |
| Git & Diffs | `gix` + `imara-diff` | `imara-diff 0.1+` |
| Testing & Bench | `cargo-nextest` + `proptest` + `insta` + `criterion` | Dev-dependencies |
