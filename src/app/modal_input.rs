//! Modal overlay input and palette behavior.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::acp::{PermissionDecision, PermissionPrompt};
use crate::platform::paths::relative_display;
use crate::ui::theme::ThemeName;

use super::action::Action;
use super::event::AppEvent;
use super::state::{MessageKind, ModalAction, ModalEntry, ModalKind, ModalState};
use super::{App, LspSync};

impl App {
    pub(super) fn open_modal(&mut self, kind: ModalKind) {
        let mut modal = ModalState::new(kind);
        self.rebuild_modal_entries(&mut modal);
        self.state.active_modal = Some(modal);
    }

    pub(super) fn show_permission(&mut self, prompt: PermissionPrompt) {
        let mut modal = ModalState::new(ModalKind::PermissionPrompt);
        modal.permission = Some(prompt);
        self.state.active_modal = Some(modal);
        self.state.focus_ai();
    }

    pub(super) fn rebuild_modal_entries(&mut self, modal: &mut ModalState) {
        let query = modal.query.trim().to_ascii_lowercase();
        match modal.kind {
            ModalKind::CommandPalette => {
                modal.entries = self
                    .state
                    .commands
                    .filter(&modal.query)
                    .into_iter()
                    .map(|spec| {
                        ModalEntry::new(
                            spec.title,
                            match spec.binding {
                                Some(binding) => format!("{} · {binding}", spec.category),
                                None => spec.category.to_string(),
                            },
                            ModalAction::Command(spec.id.to_string()),
                        )
                    })
                    .collect();
            }
            ModalKind::FileFinder => {
                let indexes = self.state.fuzzy.filter(&modal.query, 200);
                let root = self.state.workspace.root().to_path_buf();
                modal.entries = indexes
                    .into_iter()
                    .filter_map(|matched| {
                        let path = self.state.file_index.get(matched.index)?.clone();
                        let label = relative_display(&root, &path);
                        let detail = path
                            .parent()
                            .map(|parent| relative_display(&root, parent))
                            .unwrap_or_default();
                        Some(ModalEntry::new(label, detail, ModalAction::OpenFile(path)))
                    })
                    .collect();
            }
            ModalKind::ThemeSelector => {
                modal.entries = ThemeName::ALL
                    .iter()
                    .filter(|name| {
                        query.is_empty() || name.label().to_ascii_lowercase().contains(&query)
                    })
                    .map(|name| {
                        ModalEntry::new(
                            name.label(),
                            if name.is_light() { "light" } else { "dark" },
                            ModalAction::Theme(*name),
                        )
                    })
                    .collect();
            }
            ModalKind::AgentPicker => {
                modal.entries = self
                    .state
                    .acp
                    .available_agents()
                    .into_iter()
                    .filter(|profile| {
                        query.is_empty()
                            || profile.name.to_ascii_lowercase().contains(&query)
                            || profile.id.to_ascii_lowercase().contains(&query)
                    })
                    .map(|profile| {
                        ModalEntry::new(
                            profile.name.clone(),
                            profile.invocation(),
                            ModalAction::Agent(profile.id.clone()),
                        )
                    })
                    .collect();
            }
            ModalKind::Help => {
                modal.entries = self
                    .state
                    .keymap
                    .bindings()
                    .iter()
                    .filter(|(chord, command)| {
                        query.is_empty()
                            || chord.to_ascii_lowercase().contains(&query)
                            || command.to_ascii_lowercase().contains(&query)
                    })
                    .map(|(chord, command)| {
                        ModalEntry::new(
                            chord.clone(),
                            command.clone(),
                            ModalAction::Command(command.clone()),
                        )
                    })
                    .collect();
            }
            ModalKind::GlobalSearch => {
                // Entries are filled asynchronously by `SearchResults`.
            }
            ModalKind::PermissionPrompt
            | ModalKind::AiPrompt
            | ModalKind::BufferSearch
            | ModalKind::Rename
            | ModalKind::NewFile => {}
        }

        if modal.entries.is_empty() {
            modal.selected = 0;
        } else {
            modal.selected = modal.selected.min(modal.entries.len() - 1);
        }
    }

    pub(super) async fn handle_modal_key(&mut self, key: KeyEvent) {
        let Some(mut modal) = self.state.active_modal.take() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if modal.kind == ModalKind::PermissionPrompt {
            let decision = match key.code {
                KeyCode::Char('y') => Some(PermissionDecision::AllowOnce),
                KeyCode::Char('a') => Some(PermissionDecision::AllowAlways),
                KeyCode::Char('n') => Some(PermissionDecision::RejectOnce),
                KeyCode::Esc => Some(PermissionDecision::Cancelled),
                _ => None,
            };
            if let Some(decision) = decision {
                if let Some(prompt) = modal.permission.take() {
                    self.respond_permission(&prompt, decision).await;
                }
                return;
            }
            self.state.active_modal = Some(modal);
            return;
        }

        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                let kind = modal.kind;
                let query = modal.query.clone();
                let action = modal.selected_entry().map(|entry| entry.action.clone());
                if kind == ModalKind::GlobalSearch && action.is_none() {
                    modal.busy = true;
                    self.state.active_modal = Some(modal);
                    self.start_project_search(&query);
                    return;
                }
                self.activate_modal(kind, query, action).await;
                return;
            }
            KeyCode::Up => modal.select_previous(),
            KeyCode::Down => modal.select_next(),
            KeyCode::Char('p') if ctrl => modal.select_previous(),
            KeyCode::Char('n') if ctrl => modal.select_next(),
            KeyCode::Backspace => {
                modal.query.pop();
                if modal.kind != ModalKind::GlobalSearch {
                    self.rebuild_modal_entries(&mut modal);
                }
            }
            KeyCode::Char(ch) if !ctrl && !alt => {
                modal.query.push(ch);
                if modal.kind != ModalKind::GlobalSearch {
                    self.rebuild_modal_entries(&mut modal);
                }
            }
            _ => {}
        }

        self.state.active_modal = Some(modal);
    }

    async fn activate_modal(
        &mut self,
        kind: ModalKind,
        query: String,
        action: Option<ModalAction>,
    ) {
        match kind {
            ModalKind::CommandPalette | ModalKind::Help => {
                if let Some(ModalAction::Command(id)) = action {
                    self.execute_command_id(&id).await;
                }
            }
            ModalKind::FileFinder => {
                if let Some(ModalAction::OpenFile(path)) = action {
                    if self.state.open_file(&path).is_ok() {
                        self.queue_lsp_current(LspSync::Open);
                    }
                }
            }
            ModalKind::GlobalSearch => {
                if let Some(ModalAction::OpenMatch { path, line }) = action {
                    if self.state.open_file(&path).is_ok() {
                        if let Some(document) = self.state.active_doc_mut() {
                            document.goto_line(line.saturating_sub(1) as usize);
                        }
                        self.state.ensure_cursor_visible();
                        self.queue_lsp_current(LspSync::Open);
                    }
                }
            }
            ModalKind::ThemeSelector => {
                if let Some(ModalAction::Theme(name)) = action {
                    self.state.set_theme(name);
                    self.state.config.ui.theme = name.slug().to_string();
                    self.state
                        .set_status(format!("Theme: {}", name.label()), MessageKind::Info);
                }
            }
            ModalKind::AgentPicker => {
                if let Some(ModalAction::Agent(id)) = action {
                    self.start_acp(&id);
                }
            }
            ModalKind::AiPrompt => self.submit_prompt(query),
            ModalKind::BufferSearch => self.apply_buffer_search(&query),
            ModalKind::Rename => self.rename_selected(&query),
            ModalKind::NewFile => self.create_file(&query),
            ModalKind::PermissionPrompt => {}
        }
    }

    async fn execute_command_id(&mut self, id: &str) {
        match Action::from_command_id(id) {
            Some(action) => self.execute_action(action).await,
            None => self
                .state
                .set_status(format!("command not wired: {id}"), MessageKind::Warning),
        }
    }

    fn start_project_search(&mut self, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            if let Some(modal) = self.state.active_modal.as_mut() {
                modal.busy = false;
            }
            return;
        }
        if let Some(modal) = self.state.active_modal.as_mut() {
            modal.busy = true;
        }
        let root = self.state.workspace.root().to_path_buf();
        let max = self.state.config.performance.max_project_search_results;
        let sender = self.event_tx.clone();
        let search_query = query.clone();
        std::mem::drop(tokio::task::spawn_blocking(
            move || match crate::search::search_project(&root, &search_query, false, max) {
                Ok(results) => {
                    let _ = sender.blocking_send(AppEvent::SearchResults {
                        query: search_query,
                        results,
                    });
                }
                Err(error) => {
                    let _ = sender.blocking_send(AppEvent::SearchFailed(error.to_string()));
                }
            },
        ));
    }

    fn apply_buffer_search(&mut self, query: &str) {
        let query = query.trim().to_string();
        let Some(document) = self.state.active_doc() else {
            return;
        };
        let rope = document.rope.clone();
        let head = document.cursors.primary().head;
        if query.is_empty() {
            self.state.search.clear();
            return;
        }
        self.state.search.set_query(&rope, &query, false);
        let range = self
            .state
            .search
            .seek(head)
            .or_else(|| self.state.search.current_match());
        if let Some(range) = range {
            if let Some(document) = self.state.active_doc_mut() {
                document.set_cursor(range.start);
            }
            self.state.ensure_cursor_visible();
        }
        let count = self.state.search.count();
        self.state
            .set_status(format!("/{query} — {count} match(es)"), MessageKind::Info);
    }

    fn rename_selected(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(entry) = self.state.explorer.selected_entry() else {
            self.state
                .set_status("No explorer entry selected", MessageKind::Warning);
            return;
        };
        match self.state.explorer.rename(&entry.path, name) {
            Ok(path) => self
                .state
                .set_status(format!("Renamed to {}", path.display()), MessageKind::Info),
            Err(error) => self
                .state
                .set_status(format!("Rename failed: {error}"), MessageKind::Error),
        }
    }

    fn create_file(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let root = self.state.workspace.root().to_path_buf();
        let parent = match self.state.explorer.selected_entry() {
            Some(entry) if entry.is_dir => entry.path,
            Some(entry) => entry
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.clone()),
            None => root,
        };
        match self.state.explorer.create_file(&parent, name, "") {
            Ok(path) => {
                if self.state.open_file(&path).is_ok() {
                    self.queue_lsp_current(LspSync::Open);
                }
                self.state.mode = crate::app::state::EditMode::Insert;
            }
            Err(error) => self
                .state
                .set_status(format!("Create failed: {error}"), MessageKind::Error),
        }
    }

    async fn respond_permission(
        &mut self,
        prompt: &PermissionPrompt,
        decision: PermissionDecision,
    ) {
        match self.state.acp.respond_permission(prompt, decision).await {
            Ok(()) => self
                .state
                .set_status(format!("Permission {decision:?}"), MessageKind::Info),
            Err(error) => self.state.set_status(
                format!("Permission response failed: {error}"),
                MessageKind::Error,
            ),
        }
        self.state.focus_ai();
    }
}
