//! Keyboard, mouse and paste routing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::editor::text::char_at_visual_col;
use crate::editor::{Cursor, Motion};
use crate::ui::layout::compute;

use super::action::Action;
use super::state::{BottomTab, EditMode, FocusTarget, MessageKind, ModalKind};
use super::{App, LspRequest, LspSync};

impl App {
    pub(super) async fn handle_key(&mut self, key: KeyEvent) {
        tracing::debug!(code = ?key.code, modifiers = ?key.modifiers, "key event");
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        let ctrl_q =
            key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('q'));
        if !ctrl_q {
            self.state.quit_armed = false;
        }

        if self.state.active_modal.is_some() {
            self.handle_modal_key(key).await;
            return;
        }
        if self.state.mode == EditMode::Command {
            self.handle_command_mode_key(key).await;
            return;
        }
        if let Some(prefix) = self.state.pending.take() {
            self.handle_prefix_key(prefix, key).await;
            return;
        }
        if matches!(self.state.focus, FocusTarget::Bottom(BottomTab::Terminal))
            && self.state.terminal_passthrough
            && self.state.terminal.active().is_some()
        {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(
                    key.code,
                    KeyCode::Char('\\')
                        | KeyCode::Char('4')
                        | KeyCode::Char('5')
                        | KeyCode::Char(' ')
                )
            {
                self.state.terminal_passthrough = false;
                self.state.set_status(
                    "Panel navigation — 1-4 tabs · Enter resume · Esc editor",
                    MessageKind::Info,
                );
                return;
            }
            if let Some(bytes) = encode_key(key) {
                let _ = self.state.terminal.write_active(&bytes);
            }
            return;
        }

        if self.handle_global_key(key).await {
            return;
        }

        match self.state.focus {
            FocusTarget::Explorer => self.handle_explorer_key(key),
            FocusTarget::Editor => self.handle_editor_key(key),
            FocusTarget::Bottom(_) => self.handle_bottom_key(key),
            FocusTarget::Ai => self.handle_ai_key(key).await,
        }
    }

    async fn handle_global_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let code = key.code;

        if ctrl && alt && is_char(code, 'b') {
            self.execute_action(Action::ToggleAi).await;
            return true;
        }
        if ctrl && shift && is_char(code, 'p') {
            self.execute_action(Action::CommandPalette).await;
            return true;
        }
        if ctrl && shift && is_char(code, 'f') {
            self.execute_action(Action::ProjectSearch).await;
            return true;
        }
        if ctrl && shift && is_char(code, 't') {
            self.execute_action(Action::NewTerminal).await;
            return true;
        }
        if ctrl && shift && is_char(code, 'a') {
            self.execute_action(Action::ConnectAgent).await;
            return true;
        }
        if ctrl {
            match code {
                KeyCode::Char('q') => {
                    self.attempt_quit();
                    return true;
                }
                KeyCode::Char('s') => {
                    self.execute_action(Action::Save).await;
                    return true;
                }
                KeyCode::Char('p') => {
                    self.execute_action(Action::FileFinder).await;
                    return true;
                }
                KeyCode::Char('f') => {
                    self.execute_action(Action::BufferSearch).await;
                    return true;
                }
                KeyCode::Char('b') => {
                    self.execute_action(Action::ToggleExplorer).await;
                    return true;
                }
                KeyCode::Char('j') => {
                    self.execute_action(Action::ToggleBottom).await;
                    return true;
                }
                KeyCode::Char('z') => {
                    self.execute_action(Action::Undo).await;
                    return true;
                }
                KeyCode::Char('y') | KeyCode::Char('r') => {
                    self.execute_action(Action::Redo).await;
                    return true;
                }
                KeyCode::Char('a') => {
                    self.execute_action(Action::SelectAll).await;
                    return true;
                }
                KeyCode::Char('c') => {
                    self.execute_action(Action::Copy).await;
                    return true;
                }
                KeyCode::Char('v') => {
                    self.execute_action(Action::Paste).await;
                    return true;
                }
                KeyCode::Char('t') => {
                    self.execute_action(Action::NewTerminal).await;
                    return true;
                }
                KeyCode::Char('g') => {
                    self.execute_action(Action::ConnectAgent).await;
                    return true;
                }
                KeyCode::Char('o') => {
                    self.execute_action(Action::OpenConfig).await;
                    return true;
                }
                KeyCode::Char('w') if self.state.mode != EditMode::Insert => {
                    self.state.pending = Some('w');
                    return true;
                }
                KeyCode::Char(',') => {
                    self.execute_action(Action::OpenConfig).await;
                    return true;
                }
                KeyCode::Char(' ') => {
                    self.execute_action(Action::CommandPalette).await;
                    return true;
                }
                _ => {}
            }
        }

        if alt {
            if let KeyCode::Char(ch) = code {
                if let Some(digit) = ch.to_digit(10) {
                    if (1..=9).contains(&digit) {
                        let index = (digit - 1) as usize;
                        self.state.workspace.switch_to(index);
                        self.state.focus = FocusTarget::Editor;
                        return true;
                    }
                }
                match ch.to_ascii_lowercase() {
                    't' => {
                        self.execute_action(Action::NewTerminal).await;
                        return true;
                    }
                    'a' => {
                        self.execute_action(Action::ConnectAgent).await;
                        return true;
                    }
                    'c' => {
                        self.execute_action(Action::OpenConfig).await;
                        return true;
                    }
                    'p' => {
                        self.execute_action(Action::CommandPalette).await;
                        return true;
                    }
                    _ => {}
                }
            }
        }

        if code == KeyCode::F(1) {
            self.execute_action(Action::Help).await;
            return true;
        }
        if code == KeyCode::F(2) {
            self.execute_action(Action::ConnectAgent).await;
            return true;
        }
        if code == KeyCode::F(3) {
            self.execute_action(Action::NewTerminal).await;
            return true;
        }
        if code == KeyCode::F(4) {
            self.execute_action(Action::OpenConfig).await;
            return true;
        }

        if code == KeyCode::Esc {
            if self.state.mode == EditMode::Visual {
                self.state.mode = EditMode::Normal;
                if let Some(document) = self.state.active_doc_mut() {
                    document.cursors.collapse_to_heads();
                }
                return true;
            }
            if self.state.mode == EditMode::Insert {
                self.state.mode = EditMode::Normal;
                if let Some(document) = self.state.active_doc_mut() {
                    document.cursors.collapse_to_heads();
                }
                return true;
            }
        }

        let editing_insert =
            self.state.focus == FocusTarget::Editor && self.state.mode == EditMode::Insert;
        if code == KeyCode::Tab && !editing_insert {
            self.state.focus_cycle(true);
            return true;
        }
        if code == KeyCode::BackTab {
            self.state.focus_cycle(false);
            return true;
        }

        false
    }

    pub(super) fn attempt_quit(&mut self) {
        let dirty = self.state.workspace.dirty_count();
        if dirty > 0 && !self.state.quit_armed {
            self.state.quit_armed = true;
            self.state.set_status(
                format!("{dirty} unsaved buffer(s) — Ctrl+Q again to force quit, Ctrl+S to save"),
                MessageKind::Warning,
            );
        } else {
            self.state.should_quit = true;
        }
    }

    async fn handle_prefix_key(&mut self, prefix: char, key: KeyEvent) {
        match prefix {
            'w' => match key.code {
                KeyCode::Char('h') => self.state.focus = FocusTarget::Explorer,
                KeyCode::Char('l') => self.state.focus = FocusTarget::Editor,
                KeyCode::Char('j') => {
                    let tab = self.state.bottom_tab;
                    self.state.focus_bottom(tab);
                }
                KeyCode::Char('k') => self.state.focus_ai(),
                KeyCode::Char('z') => self.state.toggle_zoom(),
                KeyCode::Char('c') => self.execute_action(Action::CloseTab).await,
                _ => {}
            },
            ' ' => self.handle_leader(key).await,
            'g' => {
                if matches!(key.code, KeyCode::Char('g')) {
                    if let Some(document) = self.state.active_doc_mut() {
                        document.set_cursor(0);
                    }
                    self.state.ensure_cursor_visible();
                }
            }
            'd' => {
                if matches!(key.code, KeyCode::Char('d')) {
                    if let Some(document) = self.state.active_doc_mut() {
                        let _ = document.delete_lines();
                    }
                    self.note_edit();
                }
            }
            'y' => {
                if matches!(key.code, KeyCode::Char('y')) {
                    self.state.yank_line();
                    self.state.set_status("Yanked line", MessageKind::Info);
                }
            }
            _ => {}
        }
    }

    async fn handle_leader(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('e') => self.state.toggle_explorer(),
            KeyCode::Char('f') => self.open_modal(ModalKind::FileFinder),
            KeyCode::Char('a') => {
                self.state.toggle_ai();
                self.state.focus_ai();
            }
            KeyCode::Char('/') => self.open_modal(ModalKind::GlobalSearch),
            KeyCode::Char('c') => self.open_modal(ModalKind::CommandPalette),
            KeyCode::Char('z') => self.state.toggle_zoom(),
            KeyCode::Char('w') => self.execute_action(Action::Save).await,
            KeyCode::Char('q') => self.attempt_quit(),
            KeyCode::Char('t') => self.execute_action(Action::NewTerminal).await,
            KeyCode::Char('x') => self.execute_action(Action::CloseTab).await,
            _ => {}
        }
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        let regions = compute(
            Rect::new(0, 0, self.state.viewport.0, self.state.viewport.1),
            &self.state.layout,
        );
        let position = Position::new(mouse.column, mouse.row);

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(explorer) = regions.explorer {
                    if explorer.contains(position) {
                        self.state.focus = FocusTarget::Explorer;
                        let entries = self.state.explorer.visible_entries().len();
                        let height = explorer.height.saturating_sub(2) as usize;
                        if mouse.row > explorer.y && entries > 0 {
                            let selected = self.state.explorer.selected_index();
                            let start = if entries > height {
                                selected.saturating_sub(height.saturating_sub(1))
                            } else {
                                0
                            };
                            let index = start + (mouse.row - explorer.y - 1) as usize;
                            self.state.explorer.set_selected(index);
                        }
                        return;
                    }
                }
                if let Some(ai) = regions.ai {
                    if ai.contains(position) {
                        self.state.focus = FocusTarget::Ai;
                        return;
                    }
                }
                if let Some(bottom) = regions.bottom {
                    if bottom.contains(position) {
                        let tab = self.state.bottom_tab;
                        self.state.focus_bottom(tab);
                        return;
                    }
                }
                if regions.tab_bar.contains(position) {
                    self.select_tab_at(regions.tab_bar, mouse.column);
                    return;
                }
                if regions.editor.contains(position) {
                    self.state.focus = FocusTarget::Editor;
                    self.state.mode = EditMode::Normal;
                    self.place_cursor(regions.editor, mouse.column, mouse.row);
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if mouse.kind == MouseEventKind::ScrollUp {
                    -3isize
                } else {
                    3isize
                };
                if regions.editor.contains(position) {
                    let count = self
                        .state
                        .active_doc()
                        .map(|document| document.line_count())
                        .unwrap_or(1);
                    if let Some(document) = self.state.active_doc_mut() {
                        let next = (document.top_line as isize + delta)
                            .clamp(0, count.saturating_sub(1) as isize)
                            as usize;
                        document.top_line = next;
                    }
                } else if regions.explorer.is_some_and(|rect| rect.contains(position)) {
                    for _ in 0..3 {
                        if delta < 0 {
                            self.state.explorer.select_previous();
                        } else {
                            self.state.explorer.select_next();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn select_tab_at(&mut self, tab_bar: Rect, column: u16) {
        let mut x = tab_bar.x;
        for (index, document) in self.state.workspace.documents().iter().enumerate() {
            let label = format!(
                " {} {}{} ",
                if self.state.workspace.active_index() == Some(index) {
                    "▸"
                } else {
                    " "
                },
                document.display_name(),
                if document.dirty { " *" } else { "" }
            );
            let width = label.chars().count() as u16 + 1;
            if column >= x && column < x + width {
                self.state.workspace.switch_to(index);
                self.state.focus = FocusTarget::Editor;
                return;
            }
            x += width;
            if x >= tab_bar.right() {
                break;
            }
        }
    }

    fn place_cursor(&mut self, area: Rect, column: u16, row: u16) {
        let tab_width = self.state.config.editor.tab_width.max(1);
        let Some(document) = self.state.active_doc_mut() else {
            return;
        };
        let line_count = document.line_count().max(1);
        let digits = line_count.to_string().len() as u16;
        let gutter = digits + 2;
        let line = document
            .top_line
            .saturating_add(row.saturating_sub(area.y) as usize)
            .min(line_count - 1);
        let visual = column
            .saturating_sub(area.x + gutter)
            .saturating_add(document.left_col as u16) as usize;
        let position = char_at_visual_col(&document.rope, line, visual, tab_width);
        document.set_cursor(position);
    }

    pub(super) fn handle_explorer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.state.explorer.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.state.explorer.select_previous(),
            KeyCode::Enter | KeyCode::Char('o') => self.open_explorer_selection(),
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(entry) = self.state.explorer.selected_entry() {
                    if entry.is_dir {
                        let _ = self.state.explorer.expand(&entry.path);
                    } else {
                        self.open_explorer_selection();
                    }
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(entry) = self.state.explorer.selected_entry() {
                    if entry.is_dir && entry.expanded {
                        self.state.explorer.collapse(&entry.path);
                    } else if let Some(parent) = entry.path.parent() {
                        let parent = parent.to_path_buf();
                        self.state.explorer.select_path(&parent);
                    }
                }
            }
            KeyCode::Char('a') => self.open_modal(ModalKind::NewFile),
            KeyCode::Char('R') => self.open_modal(ModalKind::Rename),
            KeyCode::Char('r') => {
                if let Err(error) = self.state.explorer.refresh() {
                    self.state.set_status(
                        format!("Explorer refresh failed: {error}"),
                        MessageKind::Error,
                    );
                }
                self.spawn_git_refresh();
            }
            KeyCode::Char('q') | KeyCode::Esc => self.state.focus_editor(),
            _ => {}
        }
    }

    fn open_explorer_selection(&mut self) {
        let Some(entry) = self.state.explorer.selected_entry() else {
            return;
        };
        if entry.is_dir {
            let _ = self.state.explorer.toggle(&entry.path);
        } else if self.state.open_file(&entry.path).is_ok() {
            self.queue_lsp_current(LspSync::Open);
        }
    }

    pub(super) fn handle_bottom_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(ch @ '1'..='4') => {
                if let Some(digit) = ch.to_digit(10) {
                    let tab = BottomTab::from_index(digit.saturating_sub(1) as usize);
                    self.state.bottom_tab = tab;
                    self.state.focus = FocusTarget::Bottom(tab);
                    self.state.terminal_passthrough =
                        tab == BottomTab::Terminal && !self.state.terminal.is_empty();
                }
            }
            KeyCode::Char(']') => {
                let next = (self.state.bottom_tab.index() + 1) % BottomTab::ALL.len();
                let tab = BottomTab::from_index(next);
                self.state.bottom_tab = tab;
                self.state.focus = FocusTarget::Bottom(tab);
                self.state.terminal_passthrough =
                    tab == BottomTab::Terminal && !self.state.terminal.is_empty();
            }
            KeyCode::Char('[') => {
                let current = self.state.bottom_tab.index();
                let next = (current + BottomTab::ALL.len() - 1) % BottomTab::ALL.len();
                let tab = BottomTab::from_index(next);
                self.state.bottom_tab = tab;
                self.state.focus = FocusTarget::Bottom(tab);
                self.state.terminal_passthrough =
                    tab == BottomTab::Terminal && !self.state.terminal.is_empty();
            }
            KeyCode::Enter | KeyCode::Char('i') => {
                if self.state.bottom_tab == BottomTab::Terminal {
                    if self.state.terminal.is_empty() {
                        if let Err(error) = self.state.spawn_terminal() {
                            self.state.set_status(
                                format!("Terminal failed: {error}"),
                                MessageKind::Error,
                            );
                        }
                    } else {
                        self.state.terminal_passthrough = true;
                    }
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => self.state.focus_editor(),
            _ => {}
        }
    }

    pub(super) async fn handle_ai_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('i') | KeyCode::Enter => self.open_modal(ModalKind::AiPrompt),
            KeyCode::Char('a') => self.open_modal(ModalKind::AgentPicker),
            KeyCode::Char('c') => {
                self.state.ai.clear();
                self.state
                    .set_status("AI conversation cleared", MessageKind::Info);
            }
            KeyCode::Char('x') => {
                if let Some(token) = self.ai_cancel.take() {
                    token.cancel();
                }
                self.state.ai.cancelled();
                if self.state.acp.is_connected() {
                    let _ = self.state.acp.cancel();
                }
                self.state
                    .set_status("Generation cancelled", MessageKind::Info);
            }
            KeyCode::Char('q') | KeyCode::Esc => self.state.focus_editor(),
            _ => {}
        }
    }
}

impl App {
    pub(super) fn move_cursors(&mut self, motion: Motion, extend: bool) {
        let tab_width = self.state.config.editor.tab_width.max(1);
        if let Some(document) = self.state.active_doc_mut() {
            document.apply_motion(motion, extend, tab_width);
        }
        self.state.ensure_cursor_visible();
    }

    pub(super) fn add_cursor_vertical(&mut self, delta: isize) {
        let tab_width = self.state.config.editor.tab_width.max(1);
        let Some(document) = self.state.active_doc_mut() else {
            return;
        };
        let mut cursors = document.cursors.clone();
        let column = document.cursors.primary().preferred_col.unwrap_or_else(|| {
            crate::editor::text::visual_col(
                &document.rope,
                document.cursors.primary().head,
                tab_width,
            )
        });
        let line = document.rope.char_to_line(document.cursors.primary().head);
        let target_line = (line as isize + delta)
            .clamp(0, document.rope.len_lines().saturating_sub(1) as isize)
            as usize;
        let position = char_at_visual_col(&document.rope, target_line, column, tab_width);
        cursors.push(Cursor::at(position), document.rope.len_chars());
        document.cursors = cursors;
    }

    pub(super) fn queue_lsp_current(&mut self, sync: LspSync) {
        if !self.state.lsp.is_configured() {
            return;
        }
        let Some(document) = self.state.active_doc() else {
            return;
        };
        let Some(path) = document.path.clone() else {
            return;
        };
        self.lsp_request = Some(LspRequest {
            path,
            language: document.language_id.clone(),
            sync,
        });
    }

    pub(super) fn note_edit(&mut self) {
        self.lsp_dirty = true;
        self.lsp_dirty_at = Some(std::time::Instant::now());
        self.state.ensure_cursor_visible();
    }
}

pub(super) fn is_char(code: KeyCode, target: char) -> bool {
    matches!(code, KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&target))
}

/// Encode a key event into xterm VT sequences for PTY passthrough.
pub(super) fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    if ctrl {
        if let KeyCode::Char(ch) = key.code {
            let lower = ch.to_ascii_lowercase() as u8;
            let byte = match lower {
                b'a'..=b'z' => Some(lower & 0x1f),
                b'@' | b' ' => Some(0x00),
                b'[' => Some(0x1b),
                b'\\' => Some(0x1c),
                b']' => Some(0x1d),
                b'^' => Some(0x1e),
                b'_' => Some(0x1f),
                b'?' => Some(0x7f),
                _ => None,
            };
            if let Some(byte) = byte {
                return Some(vec![byte]);
            }
        }
    }

    let mut bytes = Vec::new();
    if alt {
        bytes.push(0x1b);
    }
    let sequence: Vec<u8> = match key.code {
        KeyCode::Char(ch) => ch.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => b"\x7f".to_vec(),
        KeyCode::Esc => b"\x1b".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => return None,
        },
        KeyCode::Null => b"\0".to_vec(),
        _ => return None,
    };
    bytes.extend_from_slice(&sequence);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn encodes_ctrl_letters_as_control_bytes() {
        assert_eq!(
            encode_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(vec![0x03])
        );
        assert_eq!(
            encode_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Some(vec![0x04])
        );
    }

    #[test]
    fn encodes_arrows_and_alt_prefix() {
        assert_eq!(
            encode_key(key(KeyCode::Up, KeyModifiers::NONE)),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            encode_key(key(KeyCode::Char('x'), KeyModifiers::ALT)),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn ignores_unsupported_keys() {
        assert!(encode_key(key(KeyCode::CapsLock, KeyModifiers::NONE)).is_none());
    }
}
