---
name: cross-platform-development
description: Implement OS-independent abstractions for Linux, macOS, and Windows in src/platform/ and src/terminal/, including path canonicalization, PTY/ConPTY shells, process group cleanup, atomic file saves, clipboard (native + OSC 52), and terminal quirk handling.
---

# Cross-Platform Development Skill (`cross-platform-development`)

## 1. Purpose
Guarantee identical functionality and ergonomics across **Linux**, **macOS** (`Terminal.app`, `iTerm2`, Ghostty, WezTerm), and **Windows** (`Windows Terminal`, PowerShell, CMD, ConPTY) while isolating all OS-specific code inside `src/platform/`.

## 2. Relevant Architecture
- **`src/platform/paths.rs`**: XDG / macOS Application Support / Windows AppData path resolution and Windows `\\?\` UNC prefix normalization.
- **`src/platform/shell.rs`**: Default user shell detection (`$SHELL` on Unix; `pwsh.exe` -> `powershell.exe` -> `cmd.exe` on Windows).
- **`src/platform/process.rs`**: Process group creation, `CREATE_NO_WINDOW` flags on Windows, and clean subtree termination.
- **`src/platform/clipboard.rs`**: Native clipboard (`arboard`) with automatic OSC 52 terminal escape fallback for SSH/tmux sessions.
- **`src/terminal/`**: Cross-platform PTY management via `portable-pty` + `vt100`.

## 3. Implementation Guidelines
1. **Strict `#[cfg(...)]` Isolation**:
   - Never write `#[cfg(target_os = "windows")]` or `#[cfg(unix)]` inside `editor`, `ui`, `lsp`, `acp`, or `workspace`. Expose a clean platform-agnostic function or trait in `src/platform/` instead.
2. **Path & Line Ending Discipline**:
   - Never hardcode `/` when constructing filesystem paths; use `Path::join` and `PathBuf`.
   - Normalize display/fuzzy-search relative paths with `/` via `platform::paths::to_slash_lossy(&path)`.
   - Strip `\\?\` verbatim prefixes on Windows before passing paths to `lsp-types` URLs, ACP agents, or shell commands.
   - Preserve original `LineEnding` (`LF` vs `CRLF`) when saving existing files.
3. **Windows Terminal & Console Input**:
   - Filter out `KeyEventKind::Release` events in the main input handler (Windows emits both Press and Release).
   - Provide `<Space>` leader key chords for every `Alt+` shortcut so macOS `Terminal.app` users without Option-as-Meta configured have 100% command parity.
4. **Subprocess Spawning**:
   - On Windows, configure background LSP/ACP child processes with `CREATE_NO_WINDOW` (`0x08000000`) so invisible console windows never pop up.

## 4. Code Conventions
- Use `std::path::{Path, PathBuf}` for filesystem APIs and explicit `NormalizedPath` helpers when interfacing with external protocols (LSP `file://` URIs vs ACP native absolute paths).

## 5. Dependencies
- `dirs`
- `portable-pty`
- `vt100`
- `notify`

## 6. Common Mistakes to Avoid
- **Mistake**: Using `std::fs::canonicalize` on Windows and passing the resulting `\\?\C:\...` string directly to an ACP agent or Node.js LSP server.
  - **Fix**: Always call `platform::paths::canonicalize_clean(path)`.
- **Mistake**: Using Unix-only crates (`termion`, raw `nix::unistd::forkpty`) outside `portable-pty`.
  - **Fix**: Use `crossterm` and `portable-pty` which support Unix PTYs and Windows ConPTY natively.

## 7. Testing Requirements
- Test path normalization with both Unix (`/home/user/proj/src/main.rs`) and Windows (`\\?\C:\Users\user\proj\src\main.rs`, `C:\Users\user\proj\src\main.rs`) path fixtures.
- Test `LF` and `CRLF` round-trip preservation in `Document`.

## 8. Project Documentation References
- [`docs/CROSS_PLATFORM.md`](../../../docs/CROSS_PLATFORM.md)
- [`docs/TERMINAL_INTEGRATION.md`](../../../docs/TERMINAL_INTEGRATION.md)
