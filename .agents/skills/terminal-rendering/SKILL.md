---
name: terminal-rendering
description: Optimize terminal cell-buffer rendering, DEC Mode 2026 synchronized output, viewport slicing, Unicode grapheme width calculation, and 256-color/TrueColor quantization. Use when working on src/ui/ rendering pipelines, TerminalSession lifecycle, or high-FPS editor/PTY cell blitting.
---

# Terminal Rendering Skill (`terminal-rendering`)

## 1. Purpose
Ensure tear-free, sub-2ms frame rendering across all terminal emulators by leveraging Ratatui cell diffing, DEC Mode 2026 Synchronized Output, direct `Buffer` cell blitting, Unicode grapheme width correctness, and panic-safe terminal state restoration.

## 2. Relevant Architecture
- **`TerminalSession` (`src/ui/terminal_session.rs`)**: RAII wrapper managing raw mode, alternate screen, mouse capture, bracketed paste, keyboard enhancement flags, and panic-hook restoration.
- **Direct Cell Blitters (`src/ui/editor_view.rs`, `src/ui/terminal_view.rs`)**: Write visible glyphs directly into `ratatui::buffer::Buffer` cells without intermediate `String` allocations per line.
- **Color Quantizer (`src/ui/theme.rs`)**: Converts TrueColor RGB styles to 256-color indexed styles when `COLORTERM` lacks `truecolor`/`24bit`.

## 3. Implementation Guidelines
1. **Synchronized Output Framing (DEC Mode 2026)**:
   - Wrap `terminal.draw(...)` flushes with `\x1b[?2026h` (Begin Synchronized Update) and `\x1b[?2026l` (End Synchronized Update) so modern terminals composite frames atomically.
2. **Viewport-Bounded Slicing**:
   - Never iterate over lines outside `viewport.top_line .. viewport.top_line + viewport.height`.
   - Never process columns beyond `viewport.left_col + viewport.width`.
3. **Unicode Grapheme & Width Accuracy**:
   - Iterate text via `unicode_segmentation::UnicodeSegmentation::graphemes(true)`.
   - Compute column advance via `unicode_width::UnicodeWidthStr::width(grapheme)`:
     - Tab (`\t`) expands to `tab_width - (visual_col % tab_width)` spaces.
     - Wide CJK/Emoji graphemes (width 2) advance `x` by 2 cells; if only 1 cell remains at the right border, render a truncation indicator (`›` or space) rather than overflowing the border.
     - Strip raw control characters (`0x00..=0x1F`, `0x7F`, `0x9B`) before writing to `Buffer` cells to prevent ANSI escape injection.
4. **Panic-Safe Restoration**:
   - Always keep `panic = "unwind"` in `Cargo.toml` and verify that `TerminalSession::restore()` runs in both `Drop` and the custom `std::panic::set_hook`.

## 4. Code Conventions
- Use `buf.cell_mut((x, y))` with `cell.set_symbol(grapheme).set_style(style)` in hot loops (`EditorWidget`, `TerminalWidget`).
- Pre-compute `Style` structs per theme change rather than reconstructing `Style::default().fg(...)` inside inner character loops.

## 5. Dependencies
- `ratatui` (`buffer::Buffer`, `buffer::Cell`, `style::Style`, `style::Color`)
- `crossterm` (`terminal`, `cursor`, `event`)
- `unicode-segmentation`
- `unicode-width`

## 6. Common Mistakes to Avoid
- **Mistake**: Calling `.to_string()` on an entire `ropey::Rope` or long `RopeSlice` line just to render 80 columns.
  - **Fix**: Slice the `RopeSlice` to the visible column window first, or iterate chunks lazily.
- **Mistake**: Assuming 1 `char` == 1 terminal column.
  - **Fix**: Always use `UnicodeWidthStr::width` on extended grapheme clusters.
- **Mistake**: Calling `terminal.clear()` during normal operation, causing a visible full-screen flash.
  - **Fix**: Rely on `ratatui`'s double-buffered diff engine; only call `terminal.clear()` if the user explicitly triggers `:redraw` after external corruption.

## 7. Testing Requirements
- Test wide characters (CJK `漢字`, emoji `🦀`), tabs, horizontal scrolling, and ANSI control-char sanitization in `TestBackend` cell assertions.
- Benchmark 200x60 frame rendering in `benches/` (< 1.5 ms target).

## 8. Project Documentation References
- [`docs/UI_DESIGN.md`](../../../docs/UI_DESIGN.md)
- [`docs/PERFORMANCE.md`](../../../docs/PERFORMANCE.md)
- [`docs/CROSS_PLATFORM.md`](../../../docs/CROSS_PLATFORM.md)
