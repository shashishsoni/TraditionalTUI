//! Modal overlay input and palette behavior.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::acp::{PermissionDecision, PermissionPrompt};
use crate::ai::DiffPreview;
use crate::git::GitRepository;
use crate::platform::paths::{PlatformPaths, canonicalize_clean, relative_display};
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
            self.rebuild_modal_entries(&mut modal);
        } else {
            self.rebuild_modal_entries(&mut modal);
        }
        self.state.modal_state = Some(modal);
    }

    pub(super) fn show_permission(&mut self, prompt: PermissionPrompt) {
        if let Some(diff) = DiffPreview::from_tool_call(&prompt.tool_call) {
            self.open_inline_diff(&diff);
        }
        let mut modal = ModalState::new(ModalKind::PermissionPrompt);
        modal.permission = Some(prompt);
        self.state.modal_state = Some(modal);
        self.state.focus_ai();
    }

    /// Open the full AI edit preview.
    pub(super) fn show_diff(&mut self) {
        let Some(diff) = self.state.diff_preview.clone() else {
            self.state
                .set_status("No AI edit preview available", MessageKind::Warning);
            return;
        };
        self.open_inline_diff(&diff);
    }

    fn open_inline_diff(&mut self, diff: &crate::ai::DiffPreview) {
        let inline_diff = crate::app::state::InlineDiffState::from_preview(diff);
        if let Some(index) = self.state.document_index(&diff.path) {
            self.state.workspace.switch_to(index);
        } else if diff.path.is_file() {
            let path = diff.path.clone();
            if self.state.open_file(&path).is_ok() {
                self.queue_lsp_current(LspSync::Open);
            }
        }
        // The inline rows are anchored to a document line, so scroll the
        // buffer to that line or the hunk renders at the wrong place.
        if let Some(document) = self.state.active_doc_mut() {
            document.top_line = inline_diff.top_line as usize;
        }
        self.state.inline_diff = Some(inline_diff);
        self.state.focus_editor();
        self.state.set_status(
            format!(
                "AI edit preview in editor: {} · +{} −{} — Alt+A accept · Alt+R reject · Esc dismiss",
                diff.path.display(),
                diff.added(),
                diff.removed()
            ),
            MessageKind::Info,
        );
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
            ModalKind::OpenFolder => {
                modal.entries = folder_entries(&modal.query);
            }
            ModalKind::PermissionPrompt
            | ModalKind::AiPrompt
            | ModalKind::BufferSearch
            | ModalKind::Rename
            | ModalKind::NewFile
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
        let Some(mut modal) = self.state.modal_state.take() else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if modal.kind == ModalKind::DiffPreview {
            self.handle_inline_diff_key(key).await;
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
            self.state.modal_state = Some(modal);
            return;
        }

        match key.code {
            KeyCode::Esc => return,
            KeyCode::Tab => {
                if modal.kind == ModalKind::OpenFolder {
                    if let Some(entry) = modal.selected_entry() {
                        if let ModalAction::OpenFolder(path) = &entry.action {
                            let mut completed = path.display().to_string();
                            if path.is_dir() && !completed.ends_with(std::path::MAIN_SEPARATOR) {
                                completed.push(std::path::MAIN_SEPARATOR);
                            }
                            modal.query = completed;
                            self.rebuild_modal_entries(&mut modal);
                            self.state.modal_state = Some(modal);
                            return;
                        }
                    }
                    return;
                }
            }
            KeyCode::Enter => {
                let kind = modal.kind;
                let query = modal.query.clone();
                let action = modal.selected_entry().map(|entry| entry.action.clone());
                if kind == ModalKind::GlobalSearch && action.is_none() {
                    modal.busy = true;
                    self.state.modal_state = Some(modal);
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
                self.rebuild_modal_entries(&mut modal);
            }
            KeyCode::Char(ch) if !ctrl && !alt => {
                modal.query.push(ch);
                self.rebuild_modal_entries(&mut modal);
            }
            _ => {}
        }

        self.state.modal_state = Some(modal);
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
            ModalKind::OpenFolder => {
                if let Some(action) = action {
                    match action {
                        ModalAction::OpenFolder(path) => {
                            self.open_folder_from_query(&path.display().to_string())
                        }
                        _ => self.open_folder_from_query(&query),
                    }
                } else {
                    self.open_folder_from_query(&query);
                }
            }
            ModalKind::BranchPicker => match action {
                Some(ModalAction::SwitchBranch(branch)) => self.switch_branch(&branch),
                Some(ModalAction::OpenFolder(_)) => self.open_modal(ModalKind::OpenFolder),
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
            if let Some(modal) = self.state.modal_state.as_mut() {
                modal.busy = false;
            }
            return;
        }
        if let Some(modal) = self.state.modal_state.as_mut() {
            modal.busy = true;
        }
        self.spawn_search(query);
    }

    /// Accept, reject, and navigate the inline diff. The old separate modal
    /// is gone: it only set this same inline preview and the overlay never
    /// rendered it, which is exactly the detach the user reported.
    pub(super) async fn handle_inline_diff_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(inline_diff) = self.state.inline_diff.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.state.inline_diff = None;
                self.state
                    .set_status("AI edit preview dismissed", MessageKind::Info);
            }
            KeyCode::Up => {
                inline_diff.selected = inline_diff.selected.saturating_sub(1);
                self.state.modal_state = None;
            }
            KeyCode::Down => {
                inline_diff.selected =
                    (inline_diff.selected + 1).min(inline_diff.rows.len().saturating_sub(1));
                self.state.modal_state = None;
            }
            KeyCode::PageUp => {
                inline_diff.selected = inline_diff.selected.saturating_sub(10);
                self.state.modal_state = None;
            }
            KeyCode::PageDown => {
                inline_diff.selected =
                    (inline_diff.selected + 10).min(inline_diff.rows.len().saturating_sub(1));
                self.state.modal_state = None;
            }
            KeyCode::Char('a') if alt => self.accept_inline_diff(),
            KeyCode::Char('r') if alt => self.reject_inline_diff(),
            KeyCode::Enter if ctrl => self.accept_inline_diff(),
            _ => {
                self.state.modal_state = None;
            }
        }
    }

    /// Splice the preview's added lines into the open document.
    pub(super) fn accept_inline_diff(&mut self) {
        let (path, replacement) = match (&self.state.diff_preview, &self.state.inline_diff) {
            (Some(diff), Some(inline_diff)) => {
                let mut replacement = String::new();
                for row in inline_diff.rows.iter().flatten() {
                    if row.kind != crate::ai::DiffLineKind::Removed {
                        replacement.push_str(&row.text);
                        replacement.push('\n');
                    }
                }
                (diff.path.clone(), replacement)
            }
            _ => {
                self.state
                    .set_status("No AI edit preview to accept", MessageKind::Warning);
                return;
            }
        };
        if self.open_preview_target(&path) {
            if let Some(document) = self.state.active_doc_mut() {
                let end = document.rope.len_chars();
                let _ = document.replace_ranges(
                    &[(0, end, replacement)],
                    crate::editor::history::EditKind::Replace,
                );
                self.state.inline_diff = None;
                self.note_edit();
                self.state.set_status(
                    format!("Applied AI edit to {}", path.display()),
                    MessageKind::Info,
                );
            }
        }
    }

    /// Drop the preview without touching the buffer.
    pub(super) fn reject_inline_diff(&mut self) {
        if self.state.inline_diff.is_none() {
            self.state
                .set_status("No AI edit preview to reject", MessageKind::Warning);
            return;
        }
        self.state.inline_diff = None;
        self.state
            .set_status("Rejected AI edit preview", MessageKind::Info);
    }

    fn open_preview_target(&mut self, path: &std::path::Path) -> bool {
        if let Some(index) = self.state.document_index(path) {
            self.state.workspace.switch_to(index);
            self.state.ensure_cursor_visible();
            return true;
        }
        if self.state.open_file(path).is_ok() {
            self.queue_lsp_current(LspSync::Open);
            return true;
        }
        self.state.set_status(
            format!("Cannot open preview target {}", path.display()),
            MessageKind::Error,
        );
        false
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
        self.state.sidebar_search.searching = true;
        self.state.sidebar_search.selected = 0;
        self.spawn_search(query);
    }

    fn spawn_search(&self, query: String) {
        let root = self.state.workspace.root().to_path_buf();
        let max = self
            .state
            .config
            .performance
            .max_project_search_results
            .min(self.state.sidebar_search.max_results.max(1));
        let case = if self.state.sidebar_search.case_sensitive {
            crate::search::CaseMode::Sensitive
        } else {
            crate::search::CaseMode::Smart
        };
        let whole_word = self.state.sidebar_search.whole_word;
        let sender = self.event_tx.clone();
        let search_query = query;
        std::mem::drop(tokio::task::spawn_blocking(move || {
            match crate::search::search_project_advanced(
                &root,
                &search_query,
                case,
                whole_word,
                max,
            ) {
                Ok(results) => {
                    let _ = sender.blocking_send(AppEvent::SearchResults {
                        query: search_query,
                        results,
                    });
                }
                Err(error) => {
                    let _ = sender.blocking_send(AppEvent::SearchFailed(error.to_string()));
                }
            }
        }));
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
        self.open_folder_path(&path);
    }

    pub(super) fn open_folder_path(&mut self, path: &Path) {
        if !path.is_dir() {
            self.state.set_status(
                format!("Not a directory: {}", path.display()),
                MessageKind::Error,
            );
            return;
        }
        match self.state.switch_workspace(path) {
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
            Err(error) => self
                .state
                .set_status(format!("Open folder failed: {error}"), MessageKind::Error),
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
            ModalAction::OpenFolder(self.state.workspace.root().to_path_buf()),
        ));
        modal.busy = true;
        self.state.modal_state = Some(modal);

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

fn folder_entries(query: &str) -> Vec<ModalEntry> {
    let (base, prefix) = split_folder_query(query);
    let mut dirs = list_dirs(&base);
    if !prefix.is_empty() {
        let lowered = prefix.to_ascii_lowercase();
        dirs.retain(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().starts_with(&lowered))
        });
    }
    dirs.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
    });
    dirs.truncate(200);

    let mut entries = Vec::new();
    if let Some(parent) = base.parent() {
        if parent != base {
            entries.push(ModalEntry::new(
                "… (parent)",
                parent.display().to_string(),
                ModalAction::OpenFolder(parent.to_path_buf()),
            ));
        }
    }
    for path in dirs {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        entries.push(ModalEntry::new(
            format!("▸ {name}"),
            path.display().to_string(),
            ModalAction::OpenFolder(path),
        ));
    }
    entries
}

/// Split a folder query into the directory to list and the name prefix being
/// typed, mirroring shell tab-completion behavior.
fn split_folder_query(query: &str) -> (PathBuf, String) {
    let trimmed = query.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return (current_dir_fallback(), String::new());
    }
    let expanded = expand_folder_tilde(trimmed);
    let path = PathBuf::from(&expanded);
    if expanded.ends_with(std::path::MAIN_SEPARATOR) || path.is_dir() {
        let base = canonicalize_clean(&path).unwrap_or(path);
        return (base, String::new());
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            let base = canonicalize_clean(parent).unwrap_or_else(|_| parent.to_path_buf());
            let prefix = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            (base, prefix)
        }
        _ => (current_dir_fallback(), expanded),
    }
}

fn expand_folder_tilde(input: &str) -> String {
    if input == "~" {
        return PlatformPaths::discover().home_dir().display().to_string();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return PlatformPaths::discover()
            .home_dir()
            .join(rest)
            .display()
            .to_string();
    }
    input.to_string()
}

fn current_dir_fallback() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn list_dirs(base: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Ok(entries) = std::fs::read_dir(base) else {
        return dirs;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        {
            dirs.push(path);
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_entries_lists_parent_and_children() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("alpha")).expect("mkdir");
        std::fs::create_dir(dir.path().join("beta")).expect("mkdir");
        std::fs::write(dir.path().join("file.txt"), "x").expect("write");

        let query = dir.path().display().to_string();
        let entries = folder_entries(&query);
        let labels: Vec<String> = entries.iter().map(|entry| entry.label.clone()).collect();
        assert!(labels.iter().any(|label| label.contains("parent")));
        assert!(labels.iter().any(|label| label == "▸ alpha"));
        assert!(labels.iter().any(|label| label == "▸ beta"));
        assert!(!labels.iter().any(|label| label.contains("file.txt")));

        let (base, prefix) = split_folder_query(&format!("{query}/al"));
        assert_eq!(
            base,
            dir.path()
                .canonicalize()
                .unwrap_or_else(|_| dir.path().to_path_buf())
        );
        assert_eq!(prefix, "al");
        let filtered = folder_entries(&format!("{query}/al"));
        assert!(filtered.iter().any(|entry| entry.label == "▸ alpha"));
        assert!(!filtered.iter().any(|entry| entry.label == "▸ beta"));
    }
}
