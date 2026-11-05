//! Editor key handling, Ex commands and action dispatch.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::acp::AcpManager;
use crate::ai::{ChatRole, ContextAssembler};
use crate::editor::{EditKind, Motion};
use crate::lsp::LspManager;
use crate::platform::clipboard::Clipboard;
use crate::ui::theme::ThemeName;

use super::action::Action;
use super::state::{BottomTab, EditMode, FocusTarget, MessageKind, ModalKind};
use super::{App, LspSync};

impl App {
    pub(super) fn handle_editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match self.state.mode {
            EditMode::Insert => self.handle_insert_key(key, ctrl, alt),
            EditMode::Normal => self.handle_normal_key(key, ctrl, alt),
            EditMode::Visual => self.handle_visual_key(key, ctrl, alt),
            EditMode::Command => {}
        }
    }

    fn handle_insert_key(&mut self, key: KeyEvent, ctrl: bool, alt: bool) {
        if ctrl || alt {
            return;
        }
        match key.code {
            KeyCode::Char(ch) => {
                self.insert_text(&ch.to_string(), EditKind::Insert);
            }
            KeyCode::Enter => self.insert_text("\n", EditKind::Insert),
            KeyCode::Tab => {
                let width = self.state.config.editor.tab_width.max(1);
                let column = self
                    .state
                    .active_doc()
                    .map(|document| {
                        crate::editor::text::visual_col(
                            &document.rope,
                            document.cursors.primary().head,
                            width,
                        )
                    })
                    .unwrap_or(0);
                let spaces = width - (column % width);
                self.insert_text(&" ".repeat(spaces), EditKind::Insert);
            }
            KeyCode::Backspace => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.backspace();
                }
                self.note_edit();
            }
            KeyCode::Delete => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.delete_forward();
                }
                self.note_edit();
            }
            KeyCode::Left => self.move_cursors(Motion::Left, false),
            KeyCode::Right => self.move_cursors(Motion::Right, false),
            KeyCode::Up => self.move_cursors(Motion::Up, false),
            KeyCode::Down => self.move_cursors(Motion::Down, false),
            KeyCode::Home => self.move_cursors(Motion::LineStart, false),
            KeyCode::End => self.move_cursors(Motion::LineEnd, false),
            KeyCode::PageUp => self.move_cursors(Motion::PageUp(20), false),
            KeyCode::PageDown => self.move_cursors(Motion::PageDown(20), false),
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent, ctrl: bool, alt: bool) {
        if alt {
            match key.code {
                KeyCode::Char('j') => self.add_cursor_vertical(1),
                KeyCode::Char('k') => self.add_cursor_vertical(-1),
                _ => {}
            }
            return;
        }
        if ctrl {
            match key.code {
                KeyCode::Char('d') => self.move_cursors(Motion::PageDown(20), false),
                KeyCode::Char('u') => self.move_cursors(Motion::PageUp(20), false),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('h') => self.move_cursors(Motion::Left, false),
            KeyCode::Char('j') => self.move_cursors(Motion::Down, false),
            KeyCode::Char('k') => self.move_cursors(Motion::Up, false),
            KeyCode::Char('l') => self.move_cursors(Motion::Right, false),
            KeyCode::Left => self.move_cursors(Motion::Left, false),
            KeyCode::Right => self.move_cursors(Motion::Right, false),
            KeyCode::Up => self.move_cursors(Motion::Up, false),
            KeyCode::Down => self.move_cursors(Motion::Down, false),
            KeyCode::Char('w') => self.move_cursors(Motion::WordForward, false),
            KeyCode::Char('b') => self.move_cursors(Motion::WordBackward, false),
            KeyCode::Char('e') => self.move_cursors(Motion::WordEnd, false),
            KeyCode::Char('0') | KeyCode::Home => self.move_cursors(Motion::LineStart, false),
            KeyCode::Char('$') | KeyCode::End => self.move_cursors(Motion::LineEnd, false),
            KeyCode::Char('G') => self.move_cursors(Motion::DocEnd, false),
            KeyCode::PageUp => self.move_cursors(Motion::PageUp(20), false),
            KeyCode::PageDown => self.move_cursors(Motion::PageDown(20), false),
            KeyCode::Char('i') => self.state.mode = EditMode::Insert,
            KeyCode::Char('a') => {
                self.move_cursors(Motion::Right, false);
                self.state.mode = EditMode::Insert;
            }
            KeyCode::Char('I') => {
                self.move_cursors(Motion::LineStart, false);
                self.state.mode = EditMode::Insert;
            }
            KeyCode::Char('A') => {
                self.set_cursor_to_line_end();
                self.state.mode = EditMode::Insert;
            }
            KeyCode::Char('o') => {
                self.set_cursor_to_line_end();
                self.insert_text("\n", EditKind::Insert);
            }
            KeyCode::Char('O') => {
                self.set_cursor_to_line_start();
                self.insert_text("\n", EditKind::Insert);
            }
            KeyCode::Char('x') => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.delete_forward();
                }
                self.note_edit();
            }
            KeyCode::Char('d') => self.state.pending = Some('d'),
            KeyCode::Char('y') => self.state.pending = Some('y'),
            KeyCode::Char('g') => self.state.pending = Some('g'),
            KeyCode::Char('p') => {
                self.state.paste_register();
                self.note_edit();
            }
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char('v') => self.state.mode = EditMode::Visual,
            KeyCode::Char('V') => {
                if let Some(document) = self.state.active_doc_mut() {
                    document.select_line();
                }
                self.state.mode = EditMode::Visual;
            }
            KeyCode::Char(':') => {
                self.state.mode = EditMode::Command;
                self.state.command_buffer.clear();
            }
            KeyCode::Char('/') => self.open_modal(ModalKind::BufferSearch),
            KeyCode::Char('n') => self.search_next(),
            KeyCode::Char('N') => self.search_previous(),
            KeyCode::Char(' ') => self.state.pending = Some(' '),
            _ => {}
        }
    }

    fn handle_visual_key(&mut self, key: KeyEvent, ctrl: bool, alt: bool) {
        if ctrl || alt {
            return;
        }
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.move_cursors(Motion::Left, true),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursors(Motion::Down, true),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursors(Motion::Up, true),
            KeyCode::Char('l') | KeyCode::Right => self.move_cursors(Motion::Right, true),
            KeyCode::Char('w') => self.move_cursors(Motion::WordForward, true),
            KeyCode::Char('b') => self.move_cursors(Motion::WordBackward, true),
            KeyCode::Char('e') => self.move_cursors(Motion::WordEnd, true),
            KeyCode::Char('0') | KeyCode::Home => self.move_cursors(Motion::LineStart, true),
            KeyCode::Char('$') | KeyCode::End => self.move_cursors(Motion::LineEnd, true),
            KeyCode::Char('G') => self.move_cursors(Motion::DocEnd, true),
            KeyCode::Char('y') => {
                if self.state.yank_selection() {
                    self.state.set_status("Yanked selection", MessageKind::Info);
                }
                self.finish_visual();
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.insert_text("", EditKind::Delete);
                }
                self.finish_visual();
                self.note_edit();
            }
            KeyCode::Char('c') => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.insert_text("", EditKind::Delete);
                }
                self.note_edit();
                self.state.mode = EditMode::Insert;
            }
            _ => {}
        }
    }

    fn finish_visual(&mut self) {
        self.state.mode = EditMode::Normal;
        if let Some(document) = self.state.active_doc_mut() {
            document.cursors.collapse_to_heads();
        }
    }

    pub(super) async fn handle_command_mode_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.state.mode = EditMode::Normal;
                self.state.command_buffer.clear();
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.state.command_buffer);
                self.state.mode = EditMode::Normal;
                self.execute_command_line(&line).await;
            }
            KeyCode::Backspace => {
                self.state.command_buffer.pop();
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.state.command_buffer.push(ch);
            }
            _ => {}
        }
    }

    pub(super) async fn execute_command_line(&mut self, line: &str) {
        let line = line.trim().trim_start_matches(':').trim();
        if line.is_empty() {
            return;
        }
        if let Some(rest) = line.strip_prefix("%s/") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 2 {
                let old = parts[0];
                let new = parts[1];
                let count = self
                    .state
                    .active_doc_mut()
                    .map(|document| document.replace_all(old, new, true))
                    .unwrap_or(0);
                self.note_edit();
                self.state
                    .set_status(format!("Replaced {count} occurrence(s)"), MessageKind::Info);
            }
            return;
        }

        let (command, argument) = match line.split_once(char::is_whitespace) {
            Some((command, argument)) => (command, argument.trim()),
            None => (line, ""),
        };

        match command {
            "w" | "write" => {
                if argument.is_empty() {
                    if self.state.save_active().unwrap_or(false) {
                        self.queue_lsp_current(LspSync::Save);
                    }
                } else if let Err(error) = self.state.save_active_as(std::path::Path::new(argument))
                {
                    self.state
                        .set_status(format!("Save failed: {error}"), MessageKind::Error);
                } else {
                    self.queue_lsp_current(LspSync::Save);
                }
            }
            "wa" | "wall" => match self.state.workspace.save_all() {
                Ok(()) => self
                    .state
                    .set_status("Saved all buffers", MessageKind::Info),
                Err(error) => self
                    .state
                    .set_status(format!("Save failed: {error}"), MessageKind::Error),
            },
            "q" | "quit" => self.attempt_quit(),
            "q!" => self.state.should_quit = true,
            "wq" | "x" => {
                if self.state.save_active().unwrap_or(true) {
                    self.queue_lsp_current(LspSync::Save);
                    self.state.should_quit = true;
                }
            }
            "e" | "edit" => {
                if argument.is_empty() {
                    if let Some(document) = self.state.active_doc_mut() {
                        if let Err(error) = document.reload() {
                            self.state
                                .set_status(format!("Reload failed: {error}"), MessageKind::Error);
                        } else {
                            self.queue_lsp_current(LspSync::Open);
                        }
                    }
                } else {
                    let path = self.state.workspace.root().join(argument);
                    if let Err(error) = self.state.open_file(&path) {
                        self.state
                            .set_status(format!("Open failed: {error}"), MessageKind::Error);
                    } else {
                        self.queue_lsp_current(LspSync::Open);
                    }
                }
            }
            "theme" => {
                let name = ThemeName::parse(argument);
                self.state.set_theme(name);
                self.state.config.ui.theme = name.slug().to_string();
                self.state
                    .set_status(format!("Theme: {}", name.label()), MessageKind::Info);
            }
            "acp" => self.execute_acp_command(argument).await,
            "term" | "terminal" => {
                if let Err(error) = self.state.spawn_terminal() {
                    self.state
                        .set_status(format!("Terminal failed: {error}"), MessageKind::Error);
                }
            }
            "help" => self.open_modal(ModalKind::Help),
            "trust" => self.grant_trust(),
            "git" => self.spawn_git_refresh(),
            "ai" => {
                if argument.is_empty() {
                    self.state.focus_ai();
                } else {
                    self.submit_prompt(argument.to_string());
                }
            }
            other => self
                .state
                .set_status(format!("unknown command: :{other}"), MessageKind::Warning),
        }
    }

    async fn execute_acp_command(&mut self, argument: &str) {
        let (subcommand, rest) = match argument.split_once(char::is_whitespace) {
            Some((subcommand, rest)) => (subcommand, rest.trim()),
            None => (argument, ""),
        };
        match subcommand {
            "start" if !rest.is_empty() => self.start_acp(rest),
            "stop" => {
                self.state.acp.shutdown().await;
                self.state
                    .set_status("ACP agent stopped", MessageKind::Info);
            }
            "cancel" => match self.state.acp.cancel() {
                Ok(()) => self.state.set_status("Cancel requested", MessageKind::Info),
                Err(error) => self
                    .state
                    .set_status(format!("ACP cancel failed: {error}"), MessageKind::Error),
            },
            "agents" => self.open_modal(ModalKind::AgentPicker),
            "" => self.open_modal(ModalKind::AgentPicker),
            _ => self.state.set_status(
                "usage: :acp start <id> | stop | cancel | agents",
                MessageKind::Warning,
            ),
        }
    }

    pub(super) async fn execute_action(&mut self, action: Action) {
        match action {
            Action::Save => {
                if self.state.save_active().unwrap_or(false) {
                    self.queue_lsp_current(LspSync::Save);
                }
            }
            Action::SaveAll => {
                if let Err(error) = self.state.workspace.save_all() {
                    self.state
                        .set_status(format!("Save failed: {error}"), MessageKind::Error);
                } else {
                    self.state
                        .set_status("Saved all buffers", MessageKind::Info);
                }
            }
            Action::CloseTab => self.close_active_tab(),
            Action::Quit => self.attempt_quit(),
            Action::Help => self.open_modal(ModalKind::Help),
            Action::ToggleExplorer => self.state.toggle_explorer(),
            Action::ToggleBottom => self.state.toggle_bottom(),
            Action::ToggleAi => self.state.toggle_ai(),
            Action::Zoom => self.state.toggle_zoom(),
            Action::FileFinder => self.open_modal(ModalKind::FileFinder),
            Action::CommandPalette => self.open_modal(ModalKind::CommandPalette),
            Action::ProjectSearch => self.open_modal(ModalKind::GlobalSearch),
            Action::BufferSearch => self.open_modal(ModalKind::BufferSearch),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::SelectAll => {
                if let Some(document) = self.state.active_doc_mut() {
                    document.select_all();
                }
            }
            Action::DeleteLine => {
                if let Some(document) = self.state.active_doc_mut() {
                    let _ = document.delete_lines();
                }
                self.note_edit();
            }
            Action::Copy => self.copy_to_clipboard(),
            Action::Paste => {
                self.state.paste_register();
                self.note_edit();
            }
            Action::InsertMode => self.state.mode = EditMode::Insert,
            Action::NormalMode => {
                self.state.mode = EditMode::Normal;
                if let Some(document) = self.state.active_doc_mut() {
                    document.cursors.collapse_to_heads();
                }
            }
            Action::NextTheme => self.state.cycle_theme(),
            Action::GrantTrust => self.grant_trust(),
            Action::RefreshGit => self.spawn_git_refresh(),
            Action::NewTerminal => {
                if let Err(error) = self.state.spawn_terminal() {
                    self.state
                        .set_status(format!("Terminal failed: {error}"), MessageKind::Error);
                }
                let (rows, cols) = self.state.terminal_size();
                self.state.terminal.resize_active(rows, cols);
                self.state.focus = FocusTarget::Bottom(BottomTab::Terminal);
            }
            Action::CloseTerminal => {
                self.state.terminal.close_active();
                self.state.set_status("Terminal closed", MessageKind::Info);
            }
            Action::FocusAi => self.state.focus_ai(),
            Action::ReloadFile => {
                if let Some(document) = self.state.active_doc_mut() {
                    if let Err(error) = document.reload() {
                        self.state
                            .set_status(format!("Reload failed: {error}"), MessageKind::Error);
                    } else {
                        self.queue_lsp_current(LspSync::Open);
                    }
                }
            }
        }
    }

    fn undo(&mut self) {
        let changed = self
            .state
            .active_doc_mut()
            .is_some_and(|document| document.undo());
        if changed {
            self.note_edit();
        }
    }

    fn redo(&mut self) {
        let changed = self
            .state
            .active_doc_mut()
            .is_some_and(|document| document.redo());
        if changed {
            self.note_edit();
        }
    }

    fn insert_text(&mut self, text: &str, kind: EditKind) {
        if let Some(document) = self.state.active_doc_mut() {
            let _ = document.insert_text(text, kind);
        }
        self.note_edit();
    }

    fn set_cursor_to_line_end(&mut self) {
        if let Some(document) = self.state.active_doc_mut() {
            let head = document.cursors.primary().head;
            let (_, end) = crate::editor::text::line_bounds(&document.rope, head);
            document.set_cursor(end);
        }
    }

    fn set_cursor_to_line_start(&mut self) {
        if let Some(document) = self.state.active_doc_mut() {
            let head = document.cursors.primary().head;
            let (start, _) = crate::editor::text::line_bounds(&document.rope, head);
            document.set_cursor(start);
        }
    }

    fn search_next(&mut self) {
        if let Some(range) = self.state.search.next_match() {
            if let Some(document) = self.state.active_doc_mut() {
                document.set_cursor(range.start);
            }
            self.state.ensure_cursor_visible();
        } else {
            self.state
                .set_status("No active buffer search", MessageKind::Warning);
        }
    }

    fn search_previous(&mut self) {
        if let Some(range) = self.state.search.previous_match() {
            if let Some(document) = self.state.active_doc_mut() {
                document.set_cursor(range.start);
            }
            self.state.ensure_cursor_visible();
        }
    }

    fn copy_to_clipboard(&mut self) {
        let text = if let Some(selection) = self.state.selected_text() {
            selection
        } else {
            self.state.yank_line();
            self.state.register.clone()
        };
        if text.is_empty() {
            return;
        }
        self.state.register = text.clone();
        let sequence = Clipboard::copy(&text);
        use std::io::Write;
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(sequence.as_bytes());
        let _ = stdout.flush();
        self.state.set_status(
            format!("Copied {} chars", text.chars().count()),
            MessageKind::Info,
        );
    }

    fn close_active_tab(&mut self) {
        if let Some(index) = self.state.active_index() {
            self.state.workspace.close(index);
            if self.state.workspace.is_empty() {
                self.state.workspace.scratch();
            }
            self.state.set_status("Tab closed", MessageKind::Info);
        }
    }

    pub(super) fn grant_trust(&mut self) {
        match self.state.workspace.grant_trust() {
            Ok(()) => {
                let lsp_settings = if self.state.config.lsp.enabled {
                    self.state.config.lsp.servers.clone()
                } else {
                    Default::default()
                };
                self.state.lsp = LspManager::from_settings(&lsp_settings);
                let custom_agents = if self.state.config.acp.enabled {
                    self.state.config.acp.agents.clone()
                } else {
                    Default::default()
                };
                self.state.acp = AcpManager::new(self.state.workspace.root(), &custom_agents);
                self.queue_lsp_current(LspSync::Open);
                self.state
                    .set_status("Workspace trusted", MessageKind::Info);
            }
            Err(error) => self.state.set_status(
                format!("Failed to trust workspace: {error}"),
                MessageKind::Error,
            ),
        }
    }

    /// Submit a prompt to ACP if connected, otherwise to the BYOK provider.
    pub(super) fn submit_prompt(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if self.state.acp.is_connected() {
            self.state
                .ai
                .thread_mut()
                .push(ChatRole::User, text.clone());
            match self.state.acp.send_prompt(&text) {
                Ok(()) => self.state.focus_ai(),
                Err(error) => self
                    .state
                    .set_status(format!("ACP prompt failed: {error}"), MessageKind::Error),
            }
            return;
        }
        if !self.state.ai.is_configured() {
            self.state.set_status(
                "No AI configured — set a BYOK key or connect an ACP agent (:acp agents)",
                MessageKind::Warning,
            );
            return;
        }
        let context = self.build_context();
        let (sender, receiver) = mpsc::channel(256);
        let cancel = CancellationToken::new();
        match self
            .state
            .ai
            .start_turn(text, context, sender, cancel.clone())
        {
            Ok(()) => {
                self.ai_events = Some(receiver);
                self.ai_cancel = Some(cancel);
                self.state.focus_ai();
            }
            Err(error) => self
                .state
                .set_status(format!("AI request failed: {error}"), MessageKind::Error),
        }
    }

    fn build_context(&self) -> Option<String> {
        let document = self.state.active_doc()?;
        let selection = {
            let range = document.cursors.overall_range();
            (!range.is_empty()).then(|| document.rope.slice(range).to_string())
        };
        let (line, _) = document.cursor_line_col();
        let start = line.saturating_sub(25);
        let end = (line + 26).min(document.line_count());
        let surrounding = (start..end)
            .map(|index| document.line_text_at(index))
            .collect::<Vec<_>>()
            .join("\n");
        let diagnostics: Vec<String> = self
            .state
            .lsp
            .diagnostics()
            .all()
            .map(|entry| format!("{}:{} {}", entry.uri, entry.line + 1, entry.message))
            .collect();
        let bundle = ContextAssembler::default().assemble(
            "",
            selection.as_deref(),
            Some(&surrounding),
            &diagnostics,
            &[],
            128_000,
        );
        Some(format!("{}\n\n{}", bundle.system, bundle.user))
    }

    pub(super) fn start_acp(&mut self, agent_id: &str) {
        let root = self.state.workspace.root().to_path_buf();
        let custom = self.state.config.acp.agents.clone();
        let trusted = self.state.workspace.is_trusted();
        let placeholder = AcpManager::new(&root, &custom);
        let mut manager = std::mem::replace(&mut self.state.acp, placeholder);
        let agent_id = agent_id.to_string();
        let tx = self.event_tx.clone();
        self.state
            .set_status(format!("Starting ACP agent {agent_id}…"), MessageKind::Info);
        tokio::spawn(async move {
            match manager.start(&agent_id, trusted).await {
                Ok(()) => {
                    if let Err(error) = manager.new_session().await {
                        let _ = tx
                            .send(super::event::AppEvent::Error(format!(
                                "ACP session failed: {error}"
                            )))
                            .await;
                    }
                    let _ = tx
                        .send(super::event::AppEvent::AcpReady(Box::new(manager)))
                        .await;
                }
                Err(error) => {
                    let _ = tx
                        .send(super::event::AppEvent::Error(format!("ACP: {error}")))
                        .await;
                }
            }
        });
    }
}
