# Terminal UI Design & Workbench Architecture (`UI_DESIGN.md`)

## 1. Workbench Spatial Layout

`ttui` implements a responsive, constraint-based 5-zone workbench inspired by VS Code and Zed, rendered via `ratatui::layout::Layout` with dynamic collapsible panels:

```
+----------------------------------------------------------------------------------------+
| Top Tab Bar / Breadcrumbs:  [Explorer] | [src/main.rs *] [src/acp/mod.rs] | [AI Agent] |
+------------------+---------------------------------------------------+-----------------+
| Left Sidebar     | Center Editor Workbench (Multi-Tab / Splits)      | Right Sidebar   |
| (24-32 cols)     | (Min 40 cols, Flex Fill)                          | (32-48 cols)    |
|                  |                                                   |                 |
| [Files] [Git]    |  1 | //! TradinationalTUI entrypoint              | [ACP: OpenCode] |
|                  |  2 | use crate::app::App;                         | Mode: Architect |
| v src/           |  3 |                                              |                 |
|   > acp/         |  4 | #[tokio::main]                               | User:           |
|   > editor/      |  5 | async fn main() -> anyhow::Result<()> {      | Add permission  |
|     main.rs    M |  6 |     App::bootstrap().await                   | gatekeeper.     |
|     lib.rs       |  7 | }                                            |                 |
|   Cargo.toml     |    |                                              | Agent:          |
|                  +----+----------------------------------------------+ [Tool: Edit]    |
|                  | Bottom Panel (Height: 8-16 rows, Collapsible)     |  src/acp/mod.rs |
|                  | [Terminal 1: zsh] [Problems (0)] [LSP] [ACP Trace]| [Approve (y)]   |
|                  | $ cargo nextest run                               | [Reject  (n)]   |
|                  |   PASS [ 0.014s] ttui::editor::tests::rope_insert | [Diff    (d)]   |
+------------------+---------------------------------------------------+-----------------+
| NORMAL | main* +2 ~1 | rust-analyzer | E:0 W:0 | ACP: OpenCode (Idle) | UTF-8 | 5:11  |
+----------------------------------------------------------------------------------------+
```

### Layout Engine & Responsive Breakpoints (`src/ui/layout.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkbenchVisibility {
    pub left_sidebar: bool,
    pub bottom_panel: bool,
    pub right_sidebar: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkbenchSizes {
    pub left_width: u16,     // Default: 28 cols (clamped 18..=50)
    pub right_width: u16,    // Default: 38 cols (clamped 26..=70)
    pub bottom_height: u16,  // Default: 12 rows (clamped 6..=30)
}
```

- **Small Terminal Auto-Collapse (< 100 columns)**:
  - If terminal width drops below 100 columns, `right_sidebar` automatically transitions to an overlay drawer when focused.
  - If terminal width drops below 75 columns, `left_sidebar` also collapses into a modal drawer so the central code editor always retains at least 50 columns of text width.
- **Zoom / Zen Mode (`Ctrl+W z` or `<Space>z`)**:
  - Temporarily hides Left, Bottom, and Right panels, dedicating 100% of the viewport above the Status Bar to the active editor buffer.

---

## 2. Focus Management & Modal State Machine

Every interactive panel is represented by a strongly-typed `FocusTarget`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusTarget {
    Explorer,
    EditorPane(usize),
    BottomPanel(BottomTab),
    AiSidebar,
    Modal(ModalKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BottomTab {
    Terminal,
    Problems,
    Output,
    AcpLogs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalKind {
    CommandPalette,
    FileFinder,
    GlobalSearch,
    SymbolPicker,
    PermissionPrompt,
    ThemeSelector,
    HelpKeymap,
}
```

### Focus Routing Rules
1. **Modal Preemption**: If `active_modal.is_some()`, all keyboard events route exclusively to the active modal (`Esc` closes the modal and restores previous focus).
2. **Terminal Passthrough**: When `FocusTarget::BottomPanel(BottomTab::Terminal)` is active and in `TerminalInputMode::Passthrough`, all keys (except the escape chord `Ctrl+Space`, `Ctrl+\`, or `Ctrl+4`) are serialized into VT/ANSI bytes and written directly to the active PTY master.
3. **Global Workbench Shortcuts**:
   - `Ctrl+B` / `<Space>e`: Toggle Left File Explorer.
   - `Ctrl+J` (where the terminal distinguishes it from Enter): Toggle Bottom Panel.
   - `Ctrl+T`: Open a new integrated terminal.
   - `Ctrl+Space`: Open Command Palette; exit terminal passthrough to panel navigation.
   - `Ctrl+G`: Open the ACP agent picker.
   - `Ctrl+O`: Create/open the global `config.toml`.
   - `Ctrl+Alt+B` / `<Space>a`: Toggle Right AI / ACP Assistant Panel.
   - `Ctrl+P` / `<Space>f`: Open Fuzzy File Finder.
   - `Ctrl+Shift+P` / `:`: Open Command Palette.
   - `Ctrl+Shift+F` / `<Space>/`: Open Global Project Search & Replace.
   - `Alt+1..9`: Switch active editor tab.

---

## 3. Keyboard-First + Optional Mouse Ergonomics

`ttui` supports two user-selectable keymap presets (`config.keymap.preset = "vim" | "vscode"`), combinable at any time:

### Vim / Helix Modal Preset (Default)
- **`NORMAL` Mode**: Navigation (`h/j/k/l`, `w/b/e`, `gg/G`, `Ctrl+d/u`), structural operations (`gd` goto definition, `gr` references, `K` hover documentation, `<Space>r` rename, `<Space>c` code action), multi-cursor (`Alt+j/k` add cursor below/above, `C` clone cursor).
- **`INSERT` Mode**: Direct text entry, auto-pairing brackets/quotes, live LSP completion popup (`Ctrl+Space`, `Tab`/`Enter` to accept, `Ctrl+n/p` cycle), inline AI ghost text (`Alt+]` accept).
- **`VISUAL` / `VISUAL LINE` / `VISUAL BLOCK` Mode**: Character, linewise, and rectangular block selections; send selection to AI/ACP via `<Space>ai`.
- **`COMMAND` Mode (`:`)**: `:w` (save), `:q` (quit), `:wq`, `:vsplit`, `:hsplit`, `:theme <name>`, `:acp connect <agent>`.

### Mouse Support (Optional, Enabled by Default)
- **Left Click**: Focus panel, switch tab, place editor cursor, expand/collapse Explorer folder, click `[Approve]` / `[Reject]` in ACP permission cards.
- **Click + Drag**: Visual text selection in Editor, or drag panel borders (`|`, `-`) to dynamically resize Left, Right, or Bottom splits.
- **Scroll Wheel**: Smooth 3-line vertical scroll in Editor, Explorer, Terminal scrollback, or AI conversation history under the mouse pointer (even without changing active keyboard focus).

---

## 4. Incremental Rendering & Dirty-Region Optimization

Terminal rendering performance is governed by four strict invariants:

1. **Zero Idle Redraws**: `terminal.draw(...)` is called **only** when `app.dirty == true`.
2. **Synchronized Output Framing**: Every frame flush wraps the diffed ANSI escape stream in DEC Mode 2026 (`\x1b[?2026h` before flush, `\x1b[?2026l` after flush) so terminal emulators (WezTerm, Kitty, Alacritty, iTerm2, Windows Terminal, Ghostty) composite the entire frame atomically without tearing.
3. **Viewport-Only Editor Rendering**:
   - If a file has 500,000 lines and the editor viewport is 50 rows high, `EditorWidget` slices `rope.lines_at(viewport.top_line)` and iterates **at most 50 lines**.
   - Lines longer than `viewport.width` horizontally slice only `viewport.left_col .. viewport.left_col + viewport.width` grapheme clusters.
4. **Style & Highlight Caching**:
   - Tree-sitter highlight spans and Git diff gutter markers are cached per `(BufferId, version, viewport_range)` and invalidated only when the buffer version or scroll offset changes.

---

## 5. Theme Engine & TrueColor / 256-Color Adaptation

`src/ui/theme.rs` defines semantic UI & syntax tokens mapped cleanly to `ratatui::style::Color`:

- **Built-in Themes**:
  - `TokyoNight` (Default Dark)
  - `CatppuccinMocha`
  - `GruvboxDark`
  - `OneDarkPro`
  - `SolarizedLight` / `CatppuccinLatte` (High-contrast Light)
- **Color Depth Detection**:
  - Automatically checks `COLORTERM=truecolor|24bit` and `TERM`.
  - If running in a 256-color environment (e.g., legacy `tmux` without `Tc` flag), automatically quantizes RGB colors to the nearest xterm-256 palette index via perceptual CIELAB distance.
