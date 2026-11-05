---
name: tui-development
description: Build and modify the TradinationalTUI workbench layout, focus navigation, modal overlays, keybindings, and Ratatui StatefulWidget components. Use when adding or updating panels, sidebars, tabs, status bar, command palette, or event loop routing in src/app/ or src/ui/.
---

# TUI Development Skill (`tui-development`)

## 1. Purpose
Guide the implementation of terminal workbench layouts, focus management, keyboard/mouse input routing, modal overlays, and Ratatui widgets in `src/app/` and `src/ui/` while maintaining strict domain separation and zero-flicker responsiveness.

## 2. Relevant Architecture
- **`src/app/`**: Owns `AppState`, `FocusTarget`, `ModalKind`, and the central `tokio::select!` event loop.
- **`src/ui/`**: Pure presentation layer (`UiRenderer::render(frame: &mut Frame, state: &AppState)`). Computes responsive `LayoutRegions` and renders each zone (`Explorer`, `Editor`, `BottomPanel`, `AiSidebar`, `StatusBar`, `Modal`).
- **Unidirectional Flow**: Input events (`crossterm::event::Event`) -> `Keymap::resolve` -> `Action` -> `App::dispatch_action(&mut self.state)` -> `self.dirty = true` -> `UiRenderer::render`.

## 3. Implementation Guidelines
1. **Pure Render Function**:
   - `ui::render` takes `&AppState` (immutable reference) or localized `StatefulWidget` view states. Never perform file I/O, network calls, or subprocess spawning inside `render()`.
2. **Responsive Layout Breakpoints**:
   - Always calculate layout rectangles via `ui::layout::compute_workbench_layout(area, &state.layout)`:
     - When `area.width < 100`, collapse `right_sidebar` into an overlay drawer when focused.
     - When `area.width < 75`, collapse `left_sidebar` into an overlay drawer so the center editor always preserves >= 50 columns.
3. **Focus Ring Discipline**:
   - Check `state.active_modal` first when routing keyboard input.
   - When `FocusTarget::BottomPanel(BottomTab::Terminal)` is in `Passthrough` mode, route all keys except the escape chord (`Ctrl+\` / `Ctrl+W`) to the active PTY.
4. **Windows Key Release Filtering**:
   - Always filter `KeyEvent`s with `if key.kind == KeyEventKind::Release { return Ok(()); }` so Windows Terminal does not trigger duplicate actions.

## 4. Code Conventions
- Keep individual widget renderers in dedicated submodules (`src/ui/explorer_view.rs`, `src/ui/editor_view.rs`, `src/ui/status_bar.rs`, `src/ui/ai_panel.rs`, `src/ui/palette.rs`).
- Use semantic theme styles from `state.theme` (`theme.ui.border_focused`, `theme.ui.border_unfocused`, `theme.ui.status_bar_bg`) rather than hardcoded `Color::Rgb(...)` literals.

## 5. Dependencies
- `ratatui` (`Layout`, `Constraint`, `Direction`, `Frame`, `Buffer`, `Block`, `StatefulWidget`)
- `crossterm` (`event::Event`, `event::KeyEvent`, `event::KeyEventKind`, `event::MouseEvent`)

## 6. Common Mistakes to Avoid
- **Mistake**: Importing `ratatui` or `crossterm` inside `src/editor/`, `src/lsp/`, `src/acp/`, or `src/providers/`.
  - **Fix**: Keep all `ratatui` types strictly inside `src/ui/` and `src/app/`.
- **Mistake**: Redrawing on a fixed 16ms timer when idle.
  - **Fix**: Only call `terminal.draw(...)` when `self.dirty == true`.
- **Mistake**: Calling `println!` or `eprintln!` for debug inspection.
  - **Fix**: Use `tracing::debug!` which writes to the log file and in-app `[Output]` panel.

## 7. Testing Requirements
- Every new panel or modal MUST include a headless snapshot/assertion test using `ratatui::backend::TestBackend` at both standard (`120x40`) and narrow (`70x24`) terminal dimensions.

## 8. Project Documentation References
- [`docs/UI_DESIGN.md`](../../../docs/UI_DESIGN.md)
- [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md)
