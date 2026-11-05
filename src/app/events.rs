//! Background event handling, debounced LSP sync, ACP request servicing.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc::error::TryRecvError;

use crate::acp::{AcpError, AcpEvent, AcpState, PermissionPrompt};
use crate::ai::{AiEvent, AiState};
use crate::editor::EditKind;
use crate::git::GitRepository;
use crate::lsp::{LspEvent, LspManager};
use crate::platform::paths::relative_display;

use super::event::AppEvent;
use super::state::{MessageKind, ModalAction, ModalEntry, ModalKind};
use super::{App, LspRequest, LspSync};

impl App {
    pub(super) fn start_background_tasks(&self) {
        self.spawn_file_index();
        self.spawn_git_refresh();
    }

    pub(super) fn spawn_file_index(&self) {
        let root = self.state.workspace.root().to_path_buf();
        let max = self.state.config.performance.max_file_index_entries;
        let sender = self.event_tx.clone();
        std::mem::drop(tokio::task::spawn_blocking(move || {
            let files = crate::search::index_files(&root, max);
            let _ = sender.blocking_send(AppEvent::FileIndex { root, files });
        }));
    }

    pub(super) fn spawn_git_refresh(&self) {
        let root = self.state.workspace.root().to_path_buf();
        let event_root = root.clone();
        let sender = self.event_tx.clone();
        std::mem::drop(tokio::task::spawn_blocking(
            move || match GitRepository::discover(&root) {
                Some(mut repository) => match repository.refresh() {
                    Ok(()) => {
                        let statuses = repository.statuses().clone();
                        let branch = repository.branch().map(str::to_string);
                        let _ = sender.blocking_send(AppEvent::GitRefreshed {
                            root: event_root,
                            branch,
                            statuses,
                        });
                    }
                    Err(error) => {
                        let _ = sender.blocking_send(AppEvent::GitFailed {
                            root: event_root,
                            error: error.to_string(),
                        });
                    }
                },
                None => {
                    let _ = sender.blocking_send(AppEvent::GitRefreshed {
                        root: event_root,
                        branch: None,
                        statuses: Default::default(),
                    });
                }
            },
        ));
    }

    pub(super) async fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::FileIndex { root, files } => {
                if root.as_path() != self.state.workspace.root() {
                    return;
                }
                let root = self.state.workspace.root().to_path_buf();
                let labels = files
                    .iter()
                    .map(|path| relative_display(&root, path))
                    .collect();
                self.state.file_index = files;
                self.state.fuzzy.set_items(labels);
                if self
                    .state
                    .active_modal
                    .as_ref()
                    .is_some_and(|modal| modal.kind == ModalKind::FileFinder)
                {
                    if let Some(mut modal) = self.state.active_modal.take() {
                        self.rebuild_modal_entries(&mut modal);
                        self.state.active_modal = Some(modal);
                    }
                }
            }
            AppEvent::GitRefreshed {
                root,
                branch,
                statuses,
            } => {
                if root.as_path() != self.state.workspace.root() {
                    return;
                }
                match self.state.git.as_mut() {
                    Some(repository) => repository.apply(branch, statuses.clone()),
                    None => {
                        if let Some(mut repository) =
                            GitRepository::discover(self.state.workspace.root())
                        {
                            repository.apply(branch, statuses.clone());
                            self.state.git = Some(repository);
                        }
                    }
                }
                self.state.explorer.set_git_statuses(statuses);
                let count = self.state.source_control_files().len();
                self.state.source_control_selected = self
                    .state
                    .source_control_selected
                    .min(count.saturating_sub(1));
            }
            AppEvent::GitGutter {
                root,
                version,
                marks,
            } => {
                if root.as_path() != self.state.workspace.root() {
                    return;
                }
                if let Some(document) = self.state.active_doc_mut() {
                    if document.version == version || document.version.saturating_sub(version) <= 4
                    {
                        let translated: Vec<(usize, crate::editor::GutterMark)> = marks
                            .into_iter()
                            .map(|(line, change)| {
                                let kind = match change {
                                    crate::git::LineChange::Added => {
                                        crate::editor::GutterMark::Added
                                    }
                                    crate::git::LineChange::Modified => {
                                        crate::editor::GutterMark::Modified
                                    }
                                    crate::git::LineChange::Removed => {
                                        crate::editor::GutterMark::Deleted
                                    }
                                };
                                (line, kind)
                            })
                            .collect();
                        document.git_diff = crate::editor::GutterDiff {
                            marks: translated,
                            version,
                        };
                    }
                }
            }
            AppEvent::GitFailed { root, error } => {
                if root.as_path() != self.state.workspace.root() {
                    return;
                }
                self.state.push_output(format!("git: {error}"));
                self.state.set_status(error, MessageKind::Warning);
            }
            AppEvent::SearchResults { query, results } => {
                if self.state.sidebar_search.busy
                    && self.state.sidebar_search.query.trim() == query.trim()
                {
                    self.state.sidebar_search.results = results.clone();
                    self.state.sidebar_search.selected = 0;
                    self.state.sidebar_search.busy = false;
                }
                let root = self.state.workspace.root().to_path_buf();
                let entries: Vec<ModalEntry> = results
                    .into_iter()
                    .map(|matched| {
                        ModalEntry::new(
                            relative_display(&root, &matched.path),
                            format!("{}: {}", matched.line, matched.text.trim()),
                            ModalAction::OpenMatch {
                                path: matched.path,
                                line: matched.line,
                            },
                        )
                    })
                    .collect();
                if let Some(modal) = self.state.active_modal.as_mut() {
                    if modal.kind == ModalKind::GlobalSearch && modal.query.trim() == query.trim() {
                        modal.entries = entries;
                        modal.selected = 0;
                        modal.busy = false;
                    }
                }
            }
            AppEvent::SearchFailed(error) => {
                self.state.sidebar_search.busy = false;
                if let Some(modal) = self.state.active_modal.as_mut() {
                    modal.busy = false;
                }
                self.state
                    .set_status(format!("Search failed: {error}"), MessageKind::Error);
            }
            AppEvent::Notice(text) => self.state.set_status(text, MessageKind::Info),
            AppEvent::Branches { branches, current } => {
                if let Some(modal) = self.state.active_modal.as_mut() {
                    if modal.kind == ModalKind::BranchPicker {
                        modal
                            .entries
                            .retain(|entry| matches!(entry.action, ModalAction::OpenFolder));
                        if branches.is_empty() {
                            modal.entries.push(ModalEntry::new(
                                "No local branches",
                                "open a git project or initialize one",
                                ModalAction::OpenFolder,
                            ));
                        }
                        for branch in branches {
                            let is_current = current.as_deref() == Some(branch.as_str());
                            modal.entries.push(ModalEntry::new(
                                if is_current {
                                    format!("● {branch}")
                                } else {
                                    format!("  {branch}")
                                },
                                if is_current { "current" } else { "local" },
                                ModalAction::SwitchBranch(branch),
                            ));
                        }
                        modal.selected = 0;
                        modal.busy = false;
                    }
                }
            }
            AppEvent::BranchesFailed(error) => {
                if let Some(modal) = self.state.active_modal.as_mut() {
                    if modal.kind == ModalKind::BranchPicker {
                        modal.busy = false;
                    }
                }
                self.state
                    .set_status(format!("Branch list failed: {error}"), MessageKind::Error);
            }
            AppEvent::Error(text) => {
                self.state.push_output(text.clone());
                self.state.set_status(text, MessageKind::Error);
            }
            AppEvent::AcpReady {
                manager,
                generation,
            } => {
                if generation != self.acp_generation {
                    return;
                }
                self.state.acp = *manager;
                self.state.focus_ai();
                let message = match self.state.acp.session_id() {
                    Some(session) => format!(
                        "ACP connected: {} (session {session})",
                        self.state.acp.agent_name().unwrap_or("agent")
                    ),
                    None => format!(
                        "ACP connected: {}",
                        self.state.acp.agent_name().unwrap_or("agent")
                    ),
                };
                self.state.set_status(message, MessageKind::Info);
            }
            AppEvent::LspReady {
                manager,
                generation,
            } => {
                if generation != self.lsp_generation {
                    return;
                }
                self.state.lsp = *manager;
                self.lsp_busy = false;
            }
        }
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.state.terminal.has_live_sessions()
            || self.state.lsp.has_pending_events()
            || self.lsp_request.is_some()
            || (self.lsp_dirty && !self.lsp_busy)
            || self.state.acp.has_pending_events()
            || matches!(self.state.acp.state(), AcpState::Busy | AcpState::Starting)
            || self.ai_events.is_some()
            || self.state.status_message.is_some()
    }

    pub(super) fn queue_git_gutter(&mut self) {
        let Some(repository) = self.state.git.as_ref() else {
            return;
        };
        let Some(document) = self.state.active_doc() else {
            return;
        };
        let Some(path) = document.path.clone() else {
            return;
        };
        let version = document.version;
        let root = self.state.workspace.root().to_path_buf();
        let marks = repository.working_tree_changes(&path);
        let sender = self.event_tx.clone();
        std::mem::drop(tokio::task::spawn_blocking(move || {
            let _ = sender.blocking_send(AppEvent::GitGutter {
                root,
                version,
                marks,
            });
        }));
    }

    /// Synchronous fallback used after edits and staging. Synchronous git
    /// lookups keep the gutter deterministic in tests and when the runtime
    /// is already saturated.
    pub(super) fn refresh_git_gutter(&mut self) -> bool {
        let (path, version) = match self.state.active_doc() {
            Some(document) => match document.path.clone() {
                Some(path) => (path, document.version),
                None => return false,
            },
            None => return false,
        };
        let Some(repository) = self.state.git.as_ref() else {
            return false;
        };
        let marks = repository.working_tree_changes(&path);
        let Some(document) = self.state.active_doc_mut() else {
            return false;
        };
        if document.version != version {
            return false;
        }
        let translated: Vec<(usize, crate::editor::GutterMark)> = marks
            .into_iter()
            .map(|(line, change)| {
                let kind = match change {
                    crate::git::LineChange::Added => crate::editor::GutterMark::Added,
                    crate::git::LineChange::Modified => crate::editor::GutterMark::Modified,
                    crate::git::LineChange::Removed => crate::editor::GutterMark::Deleted,
                };
                (line, kind)
            })
            .collect();
        if document.git_diff.marks == translated {
            return false;
        }
        document.git_diff = crate::editor::GutterDiff {
            marks: translated,
            version,
        };
        true
    }

    pub(super) async fn tick(&mut self) -> bool {
        let mut changed = false;

        if self.state.terminal.drain() > 0 {
            changed = true;
        }

        if self.refresh_git_gutter() {
            changed = true;
        }

        let lsp_events = self.state.lsp.poll_events();
        for event in lsp_events {
            changed = true;
            match event {
                LspEvent::Log { server, line } => {
                    self.state.push_output(format!("[{server}] {line}"));
                }
                LspEvent::Diagnostics { uri, entries } => {
                    self.state
                        .push_output(format!("diagnostics {uri}: {} item(s)", entries.len()));
                }
                LspEvent::StateChanged { server, state } => {
                    self.state.push_output(format!("[{server}] {state:?}"));
                }
                LspEvent::Exited { server, .. } => {
                    self.state
                        .set_status(format!("LSP server exited: {server}"), MessageKind::Warning);
                }
            }
        }

        if !self.lsp_busy {
            if let Some(request) = self.lsp_request.take() {
                self.spawn_lsp(request);
            } else if self.lsp_dirty
                && self
                    .lsp_dirty_at
                    .is_some_and(|at| at.elapsed() >= Duration::from_millis(300))
            {
                self.lsp_dirty = false;
                self.lsp_dirty_at = None;
                if let Some(request) = self.current_lsp_request(LspSync::Change) {
                    self.spawn_lsp(request);
                }
            }
        }

        let acp_events = self.state.acp.poll_events();
        if !acp_events.is_empty() {
            changed = true;
            for event in acp_events {
                self.process_acp_event(event).await;
            }
        }

        if let Some(mut receiver) = self.ai_events.take() {
            let mut keep = true;
            for _ in 0..64 {
                match receiver.try_recv() {
                    Ok(event) => {
                        self.state.ai.apply_event(&event);
                        changed = true;
                        if matches!(event, AiEvent::Completed(_) | AiEvent::Error(_)) {
                            keep = false;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        keep = false;
                        break;
                    }
                }
            }
            if keep && !matches!(self.state.ai.state(), AiState::Idle | AiState::Error) {
                self.ai_events = Some(receiver);
            } else {
                self.ai_cancel = None;
            }
        }

        if self
            .state
            .status_message
            .as_ref()
            .is_some_and(|message| message.is_expired())
        {
            self.state.status_message = None;
            changed = true;
        }

        changed
    }

    fn current_lsp_request(&self, sync: LspSync) -> Option<LspRequest> {
        let document = self.state.active_doc()?;
        let path = document.path.clone()?;
        Some(LspRequest {
            path,
            language: document.language_id.clone(),
            sync,
        })
    }

    fn spawn_lsp(&mut self, request: LspRequest) {
        if self.lsp_busy || !self.state.lsp.is_configured() {
            self.lsp_request = Some(request);
            return;
        }
        let Some(document) = self
            .state
            .workspace
            .documents()
            .iter()
            .find(|document| document.path.as_deref() == Some(request.path.as_path()))
        else {
            return;
        };
        let text = document.rope.to_string();
        let version = document.version;
        let path = request.path;
        let language = request.language;
        let sync = request.sync;
        let configs = self.state.lsp.configs().to_vec();
        let manager = std::mem::replace(&mut self.state.lsp, LspManager::new(configs));
        let root = self.state.workspace.root().to_path_buf();
        let sender = self.event_tx.clone();
        let generation = self.lsp_generation;
        self.lsp_busy = true;

        tokio::spawn(async move {
            let mut manager = manager;
            let result = match sync {
                LspSync::Open => {
                    manager
                        .did_open(&language, &root, &path, version, &text)
                        .await
                }
                LspSync::Save => manager.did_save(&language, &root, &path, Some(&text)).await,
                LspSync::Change => {
                    manager
                        .did_change(&language, &root, &path, version, &text)
                        .await
                }
            };
            if let Err(error) = result {
                let _ = sender.send(AppEvent::Error(format!("LSP: {error}"))).await;
            }
            let _ = sender
                .send(AppEvent::LspReady {
                    manager: Box::new(manager),
                    generation,
                })
                .await;
        });
    }

    async fn process_acp_event(&mut self, event: AcpEvent) {
        match event {
            AcpEvent::Notification { method, params } => {
                if method == "session/update" {
                    self.handle_session_update(&params);
                }
            }
            AcpEvent::Request { id, method, params } => match method.as_str() {
                "session/request_permission" => {
                    if let Some(prompt) = PermissionPrompt::from_request(id.clone(), &params) {
                        self.show_permission(prompt);
                    } else {
                        self.acp_error(id, -32602, "invalid permission request")
                            .await;
                    }
                }
                "fs/read_text_file" => self.handle_fs_read(id, &params).await,
                "fs/write_text_file" => self.handle_fs_write(id, &params).await,
                "terminal/create" => self.handle_terminal_create(id, &params).await,
                "terminal/output" => self.handle_terminal_output(id, &params).await,
                "terminal/wait_for_exit" => {
                    self.handle_terminal_wait(id, &params).await;
                }
                "terminal/kill" | "terminal/release" => {
                    self.handle_terminal_close(id, &params).await;
                }
                other => {
                    self.acp_error(id, -32601, &format!("unsupported agent request: {other}"))
                        .await;
                }
            },
            AcpEvent::Stderr(line) => {
                self.state.push_output(format!("[acp] {line}"));
            }
            AcpEvent::Exited(_) => {
                self.state
                    .set_status("ACP agent disconnected", MessageKind::Warning);
            }
        }
    }

    fn handle_session_update(&mut self, params: &Value) {
        let update = params.get("update").unwrap_or(params);
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "agent_message_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.state.ai.thread_mut().append_assistant(text);
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.state
                        .ai
                        .thread_mut()
                        .append_assistant(&format!("\n> {text}"));
                }
            }
            "tool_call" | "tool_call_update" => {
                let title = update
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let status = update.get("status").and_then(Value::as_str).unwrap_or("");
                self.state
                    .ai
                    .thread_mut()
                    .append_assistant(&format!("\n[tool] {title} {status}"));
            }
            "plan" => {
                if let Some(entries) = update.get("entries").and_then(Value::as_array) {
                    let text = entries
                        .iter()
                        .filter_map(|entry| {
                            let content = entry.get("content").and_then(Value::as_str)?;
                            let status = entry
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or("pending");
                            Some(format!("[{status}] {content}"))
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        self.state
                            .ai
                            .thread_mut()
                            .append_assistant(&format!("\n[plan]\n{text}"));
                    }
                }
            }
            "current_mode_update" => {
                if let Some(mode) = update.get("currentModeId").and_then(Value::as_str) {
                    self.state
                        .set_status(format!("ACP mode: {mode}"), MessageKind::Info);
                }
            }
            _ => {}
        }
        if let Some(diff) = crate::ai::DiffPreview::from_tool_call(update) {
            self.state.diff_preview = Some(diff);
        }
    }

    async fn handle_fs_read(&mut self, id: Value, params: &Value) {
        let Some(path) = params
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
        else {
            self.acp_error(id, -32602, "missing path").await;
            return;
        };
        let resolved = match self.state.acp.gatekeeper().check_read(&path) {
            Ok(path) => path,
            Err(AcpError::SensitivePath(_)) => {
                self.acp_error(id, -32001, "sensitive path requires confirmation")
                    .await;
                return;
            }
            Err(error) => {
                self.acp_error(id, -32602, &error.to_string()).await;
                return;
            }
        };
        let content = match self
            .state
            .workspace
            .documents()
            .iter()
            .find(|document| document.path.as_deref() == Some(resolved.as_path()))
        {
            Some(document) => Ok(document.rope.to_string()),
            None => std::fs::read_to_string(&resolved),
        };
        match content {
            Ok(text) => {
                if let Err(error) = self.state.acp.respond_fs_read(id, &text).await {
                    self.state.set_status(
                        format!("ACP fs read response failed: {error}"),
                        MessageKind::Error,
                    );
                }
            }
            Err(error) => {
                self.acp_error(id, -32603, &format!("read failed: {error}"))
                    .await;
            }
        }
    }

    async fn handle_fs_write(&mut self, id: Value, params: &Value) {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let content = params
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string);
        let (Some(path), Some(content)) = (path, content) else {
            self.acp_error(id, -32602, "missing path or content").await;
            return;
        };
        let resolved = match self.state.acp.gatekeeper().resolve_for_write(&path) {
            Ok(path) => path,
            Err(error) => {
                self.acp_error(id, -32602, &error.to_string()).await;
                return;
            }
        };
        if self.state.acp.gatekeeper().is_sensitive(&resolved) {
            self.acp_error(id, -32001, "sensitive path requires confirmation")
                .await;
            return;
        }
        if let Err(error) = crate::platform::fs::atomic_write(&resolved, content.as_bytes()) {
            self.acp_error(id, -32603, &format!("write failed: {error}"))
                .await;
            return;
        }
        if let Some(index) = self.state.workspace.find_by_path(&resolved) {
            if let Some(document) = self.state.workspace.document_mut(index) {
                let len = document.rope.len_chars();
                let _ = document.replace_ranges(&[(0, len, content)], EditKind::Other);
                let _ = document.save();
            }
            self.lsp_dirty = true;
            self.lsp_dirty_at = Some(std::time::Instant::now());
        }
        self.state
            .push_output(format!("agent wrote {}", resolved.display()));
        if let Err(error) = self.state.acp.respond_fs_write(id).await {
            self.state.set_status(
                format!("ACP fs write response failed: {error}"),
                MessageKind::Error,
            );
        }
    }

    async fn handle_terminal_create(&mut self, id: Value, params: &Value) {
        if !self.state.workspace.is_trusted() {
            self.acp_error(
                id,
                -32001,
                "workspace must be trusted to run agent terminals",
            )
            .await;
            return;
        }
        let Some(command) = params.get("command").and_then(Value::as_str) else {
            self.acp_error(id, -32602, "missing command").await;
            return;
        };
        let args: Vec<String> = params
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.state.workspace.root().to_path_buf());
        let cwd = match self.state.acp.gatekeeper().resolve_existing(&cwd) {
            Ok(cwd) => cwd,
            Err(error) => {
                self.acp_error(id, -32602, &error.to_string()).await;
                return;
            }
        };
        let Some(program) = crate::platform::shell::find_in_path(command) else {
            self.acp_error(id, -32602, &format!("command not found: {command}"))
                .await;
            return;
        };
        let (rows, cols) = self.state.terminal_size();
        match self.state.terminal.spawn_program(
            &program,
            &args,
            &cwd,
            rows,
            cols,
            Some(format!("ACP: {command}")),
        ) {
            Ok(terminal_id) => {
                self.state
                    .focus_bottom(crate::app::state::BottomTab::Terminal);
                let result = json!({ "terminalId": terminal_id.to_string() });
                if let Err(error) = self.state.acp.respond(id, result).await {
                    self.state.set_status(
                        format!("ACP terminal response failed: {error}"),
                        MessageKind::Error,
                    );
                }
            }
            Err(error) => {
                self.acp_error(id, -32603, &format!("spawn failed: {error}"))
                    .await;
            }
        }
    }

    async fn handle_terminal_output(&mut self, id: Value, params: &Value) {
        let Some(terminal_id) = terminal_id(params) else {
            self.acp_error(id, -32602, "missing terminalId").await;
            return;
        };
        let running = self
            .state
            .terminal
            .session_by_id(terminal_id)
            .map(|session| session.is_running())
            .unwrap_or(false);
        let output = self
            .state
            .terminal
            .screen_contents(terminal_id)
            .unwrap_or_default();
        let result = json!({
            "output": output,
            "truncated": false,
            "exitStatus": if running { Value::Null } else { json!({ "exitCode": 0 }) },
        });
        if let Err(error) = self.state.acp.respond(id, result).await {
            self.state.set_status(
                format!("ACP terminal output failed: {error}"),
                MessageKind::Error,
            );
        }
    }

    async fn handle_terminal_wait(&mut self, id: Value, params: &Value) {
        let Some(terminal_id) = terminal_id(params) else {
            self.acp_error(id, -32602, "missing terminalId").await;
            return;
        };
        let running = self
            .state
            .terminal
            .session_by_id(terminal_id)
            .map(|session| session.is_running())
            .unwrap_or(false);
        if running {
            self.acp_error(
                id,
                -32000,
                "terminal is still running; poll terminal/output",
            )
            .await;
        } else if let Err(error) = self.state.acp.respond(id, json!({ "exitCode": 0 })).await {
            self.state.set_status(
                format!("ACP terminal wait failed: {error}"),
                MessageKind::Error,
            );
        }
    }

    async fn handle_terminal_close(&mut self, id: Value, params: &Value) {
        if let Some(terminal_id) = terminal_id(params) {
            self.state.terminal.close_by_id(terminal_id);
        }
        if let Err(error) = self.state.acp.respond(id, Value::Null).await {
            self.state.set_status(
                format!("ACP terminal close failed: {error}"),
                MessageKind::Error,
            );
        }
    }

    async fn acp_error(&mut self, id: Value, code: i64, message: &str) {
        if let Err(error) = self.state.acp.respond_error(id, code, message).await {
            self.state.set_status(
                format!("ACP error response failed: {error}"),
                MessageKind::Error,
            );
        }
    }
}

fn terminal_id(params: &Value) -> Option<usize> {
    params.get("terminalId").and_then(|value| {
        value
            .as_str()
            .and_then(|text| text.parse::<usize>().ok())
            .or_else(|| value.as_u64().map(|number| number as usize))
    })
}
