# Cross-Platform Compatibility & OS Abstractions (`CROSS_PLATFORM.md`)

## 1. Supported Platforms & Target Triples

`TradinationalTUI (ttui)` targets full behavioral parity across all three major desktop operating systems:

| OS | Primary Target Triples | Supported Terminal Emulators |
|----|------------------------|------------------------------|
| **Linux** | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` | Alacritty, WezTerm, Kitty, Ghostty, Konsole, GNOME Terminal, Foot, xterm, tmux/zellij |
| **macOS** | `aarch64-apple-darwin` (Apple Silicon), `x86_64-apple-darwin` | iTerm2, Ghostty, WezTerm, Alacritty, Kitty, Apple Terminal.app |
| **Windows** | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | Windows Terminal (`wt.exe`), WezTerm, Alacritty, VS Code Integrated Terminal, WSL2 |

All OS-dependent behavior is isolated inside **`src/platform/`**. Domain and UI modules are strictly forbidden from using raw `#[cfg(target_os = ...)]` blocks directly.

---

## 2. Standard Directories (`src/platform/paths.rs`)

Resolved via the `dirs` crate with XDG Base Directory overrides supported across platforms:

| Purpose | Linux (XDG) | macOS | Windows |
|---------|-------------|-------|---------|
| **Configuration (`config.toml`, `keymap.toml`)** | `~/.config/ttui/` | `~/Library/Application Support/ttui/` (or `~/.config/ttui/`) | `%APPDATA%\ttui\config\` |
| **Data & Grammars (`grammars/`, `plugins/`)** | `~/.local/share/ttui/` | `~/Library/Application Support/ttui/data/` | `%LOCALAPPDATA%\ttui\data\` |
| **State & Session History (`state.json`)** | `~/.local/state/ttui/` | `~/Library/Application Support/ttui/state/` | `%LOCALAPPDATA%\ttui\state\` |
| **Logs (`ttui.log`)** | `~/.local/state/ttui/logs/` | `~/Library/Logs/ttui/` | `%LOCALAPPDATA%\ttui\logs\` |

*Note*: On macOS, if `~/.config/ttui/config.toml` exists, `ttui` honors it before falling back to `~/Library/Application Support/ttui/` so dotfiles users can share a single config repository between Linux and macOS.

---

## 3. Filesystem & Path Normalization

1. **Path Separators & Display**:
   - Internally uses `std::path::{Path, PathBuf}` for all OS calls.
   - For UI display (breadcrumbs, fuzzy file finder) and Git index matching, paths relative to the workspace root are normalized to forward slashes (`/`) for consistent fuzzy matching across Windows and Unix.
2. **Windows UNC Prefix Stripping (`\\?\`)**:
   - `std::fs::canonicalize` on Windows prepends verbatim UNC prefixes (`\\?\C:\Users\...`), which break many external CLI tools, Node.js LSP servers, and ACP agents.
   - `platform::paths::normalize_canonical_path()` uses `dunce::simplified()` semantics on Windows to strip `\\?\` whenever safe, ensuring standard `C:\Users\...` paths are passed to LSP (`file:///C:/...`) and ACP (`C:\Users\...`).
3. **Line Endings (`LF` vs `CRLF`)**:
   - `Document::from_reader` auto-detects whether an existing file uses `\n` (`LF`) or `\r\n` (`CRLF`) and preserves the file's original line ending style on save regardless of host OS.
4. **Atomic File Saves**:
   - On Unix: Writes to a temporary sibling file (`.filename.ttui.tmp`) with preserved mode permissions and atomically `rename()`s over the target (falling back to direct write if target is a symlink or hardlink).
   - On Windows: Uses `ReplaceFileW` / atomic rename retry backoff to handle transient file locks held by Windows Defender or background indexers.

---

## 4. Terminal Input & Rendering Quirks Across OSes

| Quirk / Issue | Platform Affected | `ttui` Mitigation Strategy |
|---------------|-------------------|----------------------------|
| **Duplicate `KeyEventKind::Release` events** | Windows Console / Kitty Protocol | `app` event filter processes **only** `KeyEventKind::Press` and `KeyEventKind::Repeat`, discarding `KeyEventKind::Release` so keystrokes never fire twice on Windows. |
| **`Alt` / `Option` Key as Meta vs Unicode** | macOS (`Terminal.app` / `iTerm2`) | Supports CSI-u / Kitty keyboard enhancement flags (`PushKeyboardEnhancementFlags`) where available, and binds `<Space>` leader equivalents for every `Alt+` shortcut. |
| **256-Color Limitation** | macOS `Terminal.app` | Detects missing `COLORTERM=truecolor` and automatically enables the perceptual CIELAB 256-color quantizer in `src/ui/theme.rs`. |
| **Clipboard Integration over SSH / Tmux** | Linux / macOS / Windows SSH | Dual clipboard backend: tries native OS clipboard (`arboard`) first, and automatically emits **OSC 52** terminal escape sequences when `$SSH_TTY` is set or native clipboard fails. |

---

## 5. Subprocess Management & Clean Termination (`src/platform/process.rs`)

- **Unix (Linux & macOS)**:
  - Child processes (LSP servers, ACP agents, task runners) are spawned in their own process group (`process_group(0)`).
  - Termination sends `SIGTERM` to the entire process group (`killpg`), waits up to 1,500 ms for clean shutdown, and sends `SIGKILL` if still alive.
- **Windows**:
  - Uses `.creation_flags(CREATE_NO_WINDOW)` when spawning background LSP and ACP processes so console popup windows never flash on screen.
  - Terminates process trees cleanly on shutdown so child language servers (`node.exe`, `rust-analyzer.exe`) never remain orphaned holding workspace directory locks.
