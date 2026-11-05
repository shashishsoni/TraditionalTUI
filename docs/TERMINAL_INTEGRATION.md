# Integrated Terminal & Shell Architecture (`TERMINAL_INTEGRATION.md`)

## 1. Overview

`src/terminal/` provides an embedded, multiplexed terminal emulator inside the Bottom Panel (`[Terminal]`) and also backs the ACP agent terminal execution interface (`terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`).

```
+-----------------------------------------------------------------------+
|                     `TerminalManager` (`src/terminal/mod.rs`)         |
|   Manages interactive user tabs + ACP agent background/visible PTYs   |
+-------------------+-----------------------------------+---------------+
                    |                                   |
                    v                                   v
      +---------------------------+       +---------------------------+
      | `PtySession` #1 (User)    |       | `PtySession` #2 (ACP Tool)|
      | - `portable_pty::MasterPty`|      | - `portable_pty::MasterPty`|
      | - `vt100::Parser` (Grid)  |       | - `vt100::Parser` + RawBuf|
      | - Async Reader/Writer     |       | - Exit Status Watcher     |
      +---------------------------+       +---------------------------+
```

---

## 2. Cross-Platform PTY Lifecycle (`portable-pty`)

`ttui` uses `portable_pty::native_pty_system()` to abstract OS-specific pseudo-terminal APIs:
- **Linux & macOS**: Uses POSIX `openpty` / `forkpty` with `TIOCSWINSZ` resize ioctls and session leader process groups (`setsid`).
- **Windows**: Uses Windows 10/11 **ConPTY (`CreatePseudoConsole`)** API, translating Windows console operations into standard VT/ANSI streams.

### Shell Auto-Detection (`src/platform/shell.rs`)
1. User override in `config.toml` (`[terminal] shell = "/bin/zsh"`).
2. **Linux / macOS**: Reads `$SHELL` environment variable; falls back to `/etc/passwd` login shell or `/bin/sh`.
3. **Windows**: Probes `pwsh.exe` (PowerShell 7+), then `powershell.exe` (Windows PowerShell), then `cmd.exe` via `$COMSPEC`.

---

## 3. VT100 / ANSI State Machine & Rendering (`vt100`)

Raw bytes read from the PTY master cannot be written directly to the host terminal stdout because they would overwrite the IDE's borders, editor panels, and status bar.

Instead:
1. A dedicated reader thread/task reads 8 KB chunks from `MasterPty::try_clone_reader()` and sends them via bounded channel to the `PtySession`.
2. `PtySession` feeds the bytes into `vt100::Parser::process(&bytes)` (configured with a 10,000-line scrollback buffer).
3. During `UiRenderer::render()`, the `TerminalWidget` inspects `vt100::Screen`:
   - Copies each visible `(row, col)` cell's character, foreground color, background color, and text modifiers (`BOLD`, `ITALIC`, `UNDERLINE`, `INVERSE`) directly into the bottom panel's `ratatui::buffer::Buffer` sub-rectangle.
   - Positions the hardware terminal cursor at the PTY cursor coordinates when the Terminal panel is focused.

---

## 4. Resize Synchronization (`SIGWINCH` / `ConPTY`)

Whenever the user resizes the host terminal window (`crossterm::event::Event::Resize`) or drags the Bottom Panel splitter:
1. `ui::layout` computes the new inner `(rows, cols)` of the Bottom Panel.
2. If `(rows, cols) != pty_session.size()`, `PtySession::resize(rows, cols)` invokes:
   - `master_pty.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })` (which delivers `SIGWINCH` on Unix or resizes the ConPTY buffer on Windows).
   - `vt100_parser.set_size(rows, cols)`.

---

## 5. Keyboard Input & Escape Chords

When the integrated terminal has focus (`FocusTarget::BottomPanel(BottomTab::Terminal)`):
- **`Passthrough` Mode (Default when focused)**:
  - All typing, arrow keys, `Ctrl+C`, `Ctrl+D`, `Ctrl+Z`, `Ctrl+R`, and `Tab` are encoded into standard xterm VT sequences and written to the PTY writer so interactive CLI programs (`htop`, `lazygit`, `cargo watch`, `python` REPL) work inside the panel.
  - **Escape Chord (`Ctrl+\` or `Ctrl+W`)**: Exits `Passthrough` mode back to `Normal` panel navigation mode, allowing the user to switch focus back to the Editor (`Ctrl+W k`), toggle the panel (`Ctrl+J`), or scroll through the 10,000-line terminal scrollback buffer using Vim motions (`j/k`, `Ctrl+u/d`, `/search`).
- **Clickable File Paths in Terminal Output**:
  - Compiler errors printed in the terminal (e.g., `--> src/editor/mod.rs:42:13`) are parsed on click or `gf` ("goto file") in Terminal Normal mode, opening `src/editor/mod.rs` at line 42, column 13 in the central Editor!
