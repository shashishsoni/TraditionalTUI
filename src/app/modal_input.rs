//! Modal overlay input and palette behavior.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::acp::{PermissionDecision, PermissionPrompt};
use crate::ai::DiffPreview;
use crate::git::GitRepository;
use crate::platform::paths::{PlatformPaths, relative_display};
use crate::ui::theme::ThemeName;

use super::action::Action;
use super::event::AppEvent;
use super::state::{MessageKind, ModalAction, ModalEntry, ModalKind, ModalState};
use super::{App, LspSync};

impl App {
    pub(super) fn open_modal(&mut self, kind: ModalKind) {
        if kind == ModalKind::BranchPicker {
            self.open_branch_picker();
            return;
        }
        let mut modal = ModalState::new(kind);
        if kind == ModalKind::OpenFolder {
            modal.query = self.state.workspace.root().display().to_string();
        }
        self.rebuild_modal_entries(&mut modal);
        self.state.active_modal = Some(modal);
    }

    pub(super) fn show_permission(&mut self, prompt: PermissionPrompt) {
        if let Some(diff) = DiffPreview::from_tool_call(&prompt.tool_call) {
            self.state.diff_preview = Some(diff);
        }
        let mut modal = ModalState::new(ModalKind::PermissionPrompt);
        modal.permission = Some(prompt);
        self.state.active_modal = Some(modal);
        self.state.focus_ai();
    }

    /// Open the full AI edit preview.
    pub(super) fn show_diff(&mut self) {
        if self.state.diff_preview.is_none() {
            self.state
                .set_status("No AI edit preview available", MessageKind::Warning);
            return;
        }
        let mut modal = ModalState::new(ModalKind::DiffPreview);
        modal.selected = 0;
        self.state.active_modal = Some(modal);
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
            | ModalKind::NewFile
            | ModalKind::OpenFolder
            | ModalKind::BranchPicker
            | ModalKind::DiffPreview => {}
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

        if modal.kind == ModalKind::DiffPreview {
            let line_count = self
                .state
                .diff_preview
                .as_ref()
                .map(|diff| diff.lines.len())
                .unwrap_or(0);
            match key.code {
                KeyCode::Esc => return,
                KeyCode::Up => modal.selected = modal.selected.saturating_sub(1),
                KeyCode::Down => {
                    modal.selected = (modal.selected + 1).min(line_count.saturating_sub(1));
                }
                KeyCode::PageUp => modal.selected = modal.selected.saturating_sub(10),
                KeyCode::PageDown => {
                    modal.selected = (modal.selected + 10).min(line_count.saturating_sub(1));
                }
                _ => {}
            }
            self.state.active_modal = Some(modal);
            return;
        }

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
            ModalKind::OpenFolder => self.open_folder_from_query(&query),
            ModalKind::BranchPicker => match action {
                Some(ModalAction::SwitchBranch(branch)) => self.switch_branch(&branch),
                Some(ModalAction::OpenFolder) => self.open_modal(ModalKind::OpenFolder),
                _ => {}
            },
            ModalKind::DiffPreview => {}
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
        self.spawn_search(query);
    }

    /// Run project search for the left Search sidebar.
    pub(super) fn start_sidebar_search(&mut self) {
        let query = self.state.sidebar_search.query.trim().to_string();
        if query.is_empty() {
            self.state
                .set_status("Type a search query", MessageKind::Warning);
            return;
        }
        self.state.sidebar_search.busy = true;
        self.state.sidebar_search.selected = 0;
        self.spawn_search(query);
    }

    fn spawn_search(&self, query: String) {
        let root = self.state.workspace.root().to_path_buf();
        let max = self.state.config.performance.max_project_search_results;
        let sender = self.event_tx.clone();
        let search_query = query;
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

    fn resolve_folder_path(&self, input: &str) -> Option<PathBuf> {
        let trimmed = input.trim().trim_matches('"').trim_matches('\'');
        if trimmed.is_empty() {
            return None;
        }
        let home = PlatformPaths::discover().home_dir().to_path_buf();
        let path = if trimmed == "~" {
            home
        } else if let Some(rest) = trimmed.strip_prefix("~/") {
            home.join(rest)
        } else {
            PathBuf::from(trimmed)
        };
        Some(if path.is_absolute() {
            path
        } else {
            self.state.workspace.root().join(path)
        })
    }

    fn open_folder_from_query(&mut self, query: &str) {
        let Some(path) = self.resolve_folder_path(query) else {
            self.state
                .set_status("Enter a folder path", MessageKind::Warning);
            return;
        };
        if !path.is_dir() {
            self.state.set_status(
                format!("Not a directory: {}", path.display()),
                MessageKind::Error,
            );
            self.open_modal(ModalKind::OpenFolder);
            if let Some(modal) = self.state.active_modal.as_mut() {
                modal.query = query.to_string();
            }
            return;
        }
        match self.state.switch_workspace(&path) {
            Ok(()) => {
                self.lsp_generation = self.lsp_generation.wrapping_add(1);
                self.acp_generation = self.acp_generation.wrapping_add(1);
                self.lsp_busy = false;
                self.lsp_request = None;
                self.lsp_dirty = false;
                self.spawn_file_index();
                self.spawn_git_refresh();
                self.queue_lsp_current(LspSync::Open);
                self.state.set_status(
                    format!("Opened folder {}", path.display()),
                    MessageKind::Info,
                );
            }
            Err(error) => {
                self.state
                    .set_status(format!("Open folder failed: {error}"), MessageKind::Error);
                self.open_modal(ModalKind::OpenFolder);
                if let Some(modal) = self.state.active_modal.as_mut() {
                    modal.query = query.to_string();
                }
            }
        }
    }

    /// `:open <path>` opens a file or switches the workspace for directories.
    pub(super) fn open_folder_from_query_command(&mut self, input: &str) {
        if let Some(path) = self.resolve_folder_path(input) {
            if path.is_file() {
                if self.state.open_file(&path).is_ok() {
                    self.queue_lsp_current(LspSync::Open);
                }
                return;
            }
        }
        self.open_folder_from_query(input);
    }

    pub(super) fn open_branch_picker(&mut self) {
        let mut modal = ModalState::new(ModalKind::BranchPicker);
        modal.entries.push(ModalEntry::new(
            "⌂ Open Folder…",
            "switch project root",
            ModalAction::OpenFolder,
        ));
        modal.busy = true;
        self.state.active_modal = Some(modal);

        let root = self.state.workspace.root().to_path_buf();
        let sender = self.event_tx.clone();
        std::mem::drop(tokio::task::spawn_blocking(move || {
            let result = match GitRepository::discover(&root) {
                Some(mut repository) => {
                    let _ = repository.refresh();
                    repository
                        .list_branches()
                        .map(|branches| (branches, repository.branch().map(str::to_string)))
                }
                None => Ok((Vec::new(), None)),
            };
            match result {
                Ok((branches, current)) => {
                    let _ = sender.blocking_send(AppEvent::Branches { branches, current });
                }
                Err(error) => {
                    let _ = sender.blocking_send(AppEvent::BranchesFailed(error.to_string()));
                }
            }
        }));
    }

    fn switch_branch(&mut self, branch: &str) {
        let name = branch.to_string();
        let root = self.state.workspace.root().to_path_buf();
        let event_root = root.clone();
        let sender = self.event_tx.clone();
        self.state
            .set_status(format!("Switching to {name}…"), MessageKind::Info);
        std::mem::drop(tokio::task::spawn_blocking(
            move || match GitRepository::discover(&root) {
                Some(mut repository) => {
                    let result = repository
                        .switch_branch(&name)
                        .and_then(|()| repository.refresh());
                    match result {
                        Ok(()) => {
                            let branch = repository.branch().map(str::to_string);
                            let statuses = repository.statuses().clone();
                            let _ = sender.blocking_send(AppEvent::GitRefreshed {
                                root: event_root,
                                branch,
                                statuses,
                            });
                            let _ = sender
                                .blocking_send(AppEvent::Notice(format!("Switched to {name}")));
                        }
                        Err(error) => {
                            let _ = sender.blocking_send(AppEvent::Error(format!(
                                "Branch switch failed: {error}"
                            )));
                        }
                    }
                }
                None => {
                    let _ = sender.blocking_send(AppEvent::Error(
                        "Branch switch failed: not a git repository".to_string(),
                    ));
                }
            },
        ));
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
