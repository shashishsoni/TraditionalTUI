# AGENTS.md — AI Coding Agent Implementation Guide & Repository Rules

Welcome to **TradinationalTUI (`ttui`)**, a lightweight, cross-platform native Terminal User Interface (TUI) IDE written in Rust.

This file is the authoritative rulebook for any AI coding agent working in this repository. Read this file and the referenced `docs/` specifications **before** modifying or adding code.

---

## 1. Essential Architecture & Documentation Index

Before implementing features in any subsystem, consult its specification in `docs/` and activate the matching skill in `.agents/skills/`:

| Domain | Documentation Spec | Matching Skill (`.agents/skills/`) |
|--------|--------------------|------------------------------------|
| Project Vision & UX | [`docs/PROJECT_OVERVIEW.md`](./docs/PROJECT_OVERVIEW.md) | — |
| Tech Stack & Crates | [`docs/TECH_STACK.md`](./docs/TECH_STACK.md), [`docs/DECISIONS.md`](./docs/DECISIONS.md) | — |
| System Architecture | [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) | `tui-development` |
| Layout & Rendering | [`docs/UI_DESIGN.md`](./docs/UI_DESIGN.md) | `tui-development`, `terminal-rendering` |
| Rope & Multi-Cursor Editor | [`docs/EDITOR_ENGINE.md`](./docs/EDITOR_ENGINE.md) | `editor-engine-development` |
| Agent Client Protocol (ACP) | [`docs/ACP_INTEGRATION.md`](./docs/ACP_INTEGRATION.md) | `acp-protocol-integration` |
| BYOK AI Providers | [`docs/AI_PROVIDER_ARCHITECTURE.md`](./docs/AI_PROVIDER_ARCHITECTURE.md) | `ai-provider-integration` |
| Language Server Protocol | [`docs/LSP_INTEGRATION.md`](./docs/LSP_INTEGRATION.md) | `lsp-integration` |
| Integrated Terminal (PTY) | [`docs/TERMINAL_INTEGRATION.md`](./docs/TERMINAL_INTEGRATION.md) | `cross-platform-development` |
| Cross-Platform OS Rules | [`docs/CROSS_PLATFORM.md`](./docs/CROSS_PLATFORM.md) | `cross-platform-development` |
| Performance & Benchmarks | [`docs/PERFORMANCE.md`](./docs/PERFORMANCE.md) | `performance-optimization` |
| Security & Permissions | [`docs/SECURITY.md`](./docs/SECURITY.md) | `acp-protocol-integration` |
| Testing & Quality | [`docs/TESTING.md`](./docs/TESTING.md) | `testing-and-code-quality` |
| Build & Toolchain | [`docs/BUILD_AND_INSTALL.md`](./docs/BUILD_AND_INSTALL.md) | — |
| Phased Roadmap | [`docs/ROADMAP.md`](./docs/ROADMAP.md) | — |

---

## 2. Strict Architectural & Module Boundary Rules

The project is divided into 16 modular subsystems under `src/`:
`app`, `ui`, `editor`, `workspace`, `explorer`, `terminal`, `lsp`, `git`, `search`, `commands`, `plugins`, `ai`, `acp`, `providers`, `config`, `platform`.

1. **No Terminal UI Imports in Domain Engines**:
   - `editor`, `lsp`, `acp`, `providers`, `search`, `git`, `terminal`, `config`, and `platform` **MUST NEVER** import `ratatui` or `crossterm`.
   - Only `ui` and `app` may touch terminal rendering and terminal input events.
2. **Single-Owner State + Typed Message Channels**:
   - Do not wrap `AppState` or `Document` in `Arc<Mutex<...>>`.
   - Background tasks receive $O(1)$ `ropey::Rope` clones and send results back to the main loop via bounded `tokio::sync::mpsc::Sender<AppEvent>`.
3. **ACP and BYOK Provider Separation**:
   - Keep `src/acp/` (JSON-RPC 2.0 stdio coding agents via `agent-client-protocol`) completely separate from `src/providers/` (stateless HTTPS/SSE BYOK LLM providers).
   - Never hardcode vendor-specific agent hacks inside `editor` or `ui`.
4. **Platform Isolation**:
   - Never scatter `#[cfg(target_os = "windows")]` or `#[cfg(unix)]` across feature modules. Place all OS-specific paths, process group management, clipboard, and shell detection inside `src/platform/`.
5. **Graceful Zero-AI Operation**:
   - The IDE must start, edit, run LSP, and use the integrated terminal with **zero** errors or background polling when no ACP agent or BYOK API key is configured.

---

## 3. Non-Negotiable Code Quality & Safety Rules

1. **Zero `stdout` / `stderr` Contamination**:
   - **NEVER** use `println!`, `eprintln!`, or `dbg!` anywhere in library or runtime TUI code (enforced by `#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]`). Writing to stdout/stderr while in raw mode corrupts the terminal cell buffer.
   - Always use `tracing::{debug, info, warn, error}`.
   - Always pipe `stdin`, `stdout`, and `stderr` (`Stdio::piped()`) of spawned child processes (LSP servers, ACP agents, git helpers).
2. **No Silent Placeholders**:
   - Never write fake stub implementations that silently return `Ok(())` and pretend a feature works.
   - If a phase feature is not yet wired, return a typed `Error::NotImplemented("...")` or surface an explicit status bar message.
3. **Error Handling & Panic Safety**:
   - Use `thiserror` for domain error enums and `anyhow::Result` at the top-level application boundary.
   - Avoid `.unwrap()` and `.expect()` in runtime paths; handle errors explicitly.
   - Never set `panic = "abort"` in `Cargo.toml`—`TerminalSession`'s `Drop` guard and panic hook rely on unwinding to restore the user's terminal state.
4. **Small, Focused Files and Functions**:
   - Keep functions under ~60 lines and files under ~500 lines. Split submodules cleanly (`mod.rs` + focused files).

---

## 4. Verification Checklist (Run Before Completing Any Task)

Every code change must pass all four checks before completion:

```bash
# 1. Formatting check
cargo fmt --all -- --check

# 2. Strict Clippy lints (zero warnings allowed)
cargo clippy --all-targets -- -D warnings

# 3. Full unit, property, and headless UI test suite
cargo nextest run --all-targets || cargo test --all-targets

# 4. Verify benchmark compilation
cargo bench --no-run
```
