//! Authoritative workbench state owned by the app loop.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::acp::{AcpManager, PermissionPrompt};
use crate::ai::AiCoordinator;
use crate::commands::{CommandRegistry, Keymap};
use crate::config::Config;
use crate::editor::{BufferSearch, Document};
use crate::explorer::FileExplorer;
use crate::git::GitRepository;
use crate::lsp::LspManager;
use crate::providers::{
    AiProvider, CredentialStore, ProviderConfig, ProviderKind, provider_from_config,
};
use crate::search::FuzzyPicker;
use crate::terminal::TerminalManager;
use crate::ui::layout::{LayoutState, compute};
use crate::ui::theme::{ColorDepth, Theme, ThemeName};
use crate::workspace::Workspace;
use crate::{Result, TtuiError};

/// Which workbench zone owns keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Explorer,
    Editor,
    Bottom(BottomTab),
    Ai,
}

/// Tabs inside the bottom panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Terminal,
    Problems,
    Output,
    AcpLogs,
}

impl BottomTab {
    pub const ALL: [BottomTab; 4] = [
        BottomTab::Terminal,
        BottomTab::Problems,
        BottomTab::Output,
        BottomTab::AcpLogs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Problems => "Problems",
            Self::Output => "Output",
            Self::AcpLogs => "ACP Logs",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Terminal => 0,
            Self::Problems => 1,
            Self::Output => 2,
            Self::AcpLogs => 3,
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }
}

/// Editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Normal,
    Insert,
    Visual,
    Command,
}

impl EditMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
        }
    }
}

/// Modal overlay kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    CommandPalette,
    FileFinder,
    GlobalSearch,
    Help,
    ThemeSelector,
    PermissionPrompt,
    AgentPicker,
    AiPrompt,
    BufferSearch,
    Rename,
    NewFile,
}

impl ModalKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::CommandPalette => "Command Palette",
            Self::FileFinder => "Go to File",
            Self::GlobalSearch => "Search in Project",
            Self::Help => "Keymap Help",
            Self::ThemeSelector => "Select Theme",
            Self::PermissionPrompt => "Permission Required",
            Self::AgentPicker => "Connect ACP Agent",
            Self::AiPrompt => "Ask AI",
            Self::BufferSearch => "Search Buffer",
            Self::Rename => "Rename",
            Self::NewFile => "New File",
        }
    }

    /// Whether the modal edits a free-text input line.
    pub fn has_input(self) -> bool {
        !matches!(
            self,
            Self::Help | Self::PermissionPrompt | Self::ThemeSelector
        )
    }

    pub fn has_list(self) -> bool {
        !matches!(
            self,
            Self::AiPrompt | Self::BufferSearch | Self::Rename | Self::NewFile
        )
    }
}

/// Action triggered by a modal entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalAction {
    Command(String),
    OpenFile(PathBuf),
    OpenMatch { path: PathBuf, line: u64 },
    Theme(ThemeName),
    Agent(String),
}

/// One selectable modal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalEntry {
    pub label: String,
    pub detail: String,
    pub action: ModalAction,
}

impl ModalEntry {
    pub fn new(label: impl Into<String>, detail: impl Into<String>, action: ModalAction) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            action,
        }
    }
}

/// Active modal overlay.
#[derive(Debug, Clone)]
pub struct ModalState {
    pub kind: ModalKind,
    pub query: String,
    pub selected: usize,
    pub entries: Vec<ModalEntry>,
    pub permission: Option<PermissionPrompt>,
    pub busy: bool,
}

impl ModalState {
    pub fn new(kind: ModalKind) -> Self {
        Self {
            kind,
            query: String::new(),
            selected: 0,
            entries: Vec::new(),
            permission: None,
            busy: false,
        }
    }

    pub fn selected_entry(&self) -> Option<&ModalEntry> {
        self.entries.get(self.selected)
    }

    pub fn select_next(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

/// Status message severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Warning,
    Error,
}

/// Transient status bar message.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub kind: MessageKind,
    pub created: Instant,
    pub ttl: Duration,
}

impl StatusMessage {
    pub fn new(text: impl Into<String>, kind: MessageKind, ttl: Duration) -> Self {
        Self {
            text: text.into(),
            kind,
            created: Instant::now(),
            ttl,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.created.elapsed() >= self.ttl
    }
}

/// The single-owner workbench state.
pub struct AppState {
    pub config: Config,
    pub workspace: Workspace,
    pub explorer: FileExplorer,
    pub commands: CommandRegistry,
    pub keymap: Keymap,
    pub layout: LayoutState,
    pub theme: Theme,
    pub mode: EditMode,
    pub focus: FocusTarget,
    pub bottom_tab: BottomTab,
    pub active_modal: Option<ModalState>,
    pub status_message: Option<StatusMessage>,
    pub should_quit: bool,
    pub file_index: Vec<PathBuf>,
    pub fuzzy: FuzzyPicker,
    pub terminal: TerminalManager,
    pub git: Option<GitRepository>,
    pub lsp: LspManager,
    pub acp: AcpManager,
    pub ai: AiCoordinator,
    pub credentials: CredentialStore,
    pub output: VecDeque<String>,
    pub search: BufferSearch,
    pub register: String,
    pub pending: Option<char>,
    pub command_buffer: String,
    pub viewport: (u16, u16),
    pub quit_armed: bool,
    pub terminal_passthrough: bool,
}

impl AppState {
    /// Bootstrap config, workspace and subsystem managers.
    pub fn bootstrap(root: &Path, initial_file: Option<&Path>) -> Result<Self> {
        crate::platform::paths::PlatformPaths::discover()
            .ensure_dirs()
            .map_err(TtuiError::Io)?;
        let config =
            crate::config::load(root).map_err(|error| TtuiError::Config(error.to_string()))?;
        let mut workspace = Workspace::new(root)?;
        let mut notes = Vec::new();

        if let Some(file) = initial_file {
            if file.is_file() {
                if let Err(error) = workspace.open(file) {
                    notes.push(format!("could not open {}: {error}", file.display()));
                }
            }
        }
        if workspace.is_empty() {
            match workspace.load_session() {
                Ok(_) => {}
                Err(error) => notes.push(format!("session restore failed: {error}")),
            }
        }
        if workspace.is_empty() {
            workspace.scratch();
            notes.push(
                "Welcome to ttui — type to edit, Esc for normal mode, Ctrl+Shift+P for commands."
                    .to_string(),
            );
        }

        let explorer = FileExplorer::new(root)?;
        let mut state = Self::assemble(config, workspace, explorer);
        state.output.extend(notes);
        Ok(state)
    }

    fn assemble(config: Config, workspace: Workspace, explorer: FileExplorer) -> Self {
        let trusted = workspace.is_trusted();
        let lsp_settings = if config.lsp.enabled && trusted {
            config.lsp.servers.clone()
        } else {
            Default::default()
        };
        let lsp = LspManager::from_settings(&lsp_settings);
        let custom_agents = if config.acp.enabled && trusted {
            config.acp.agents.clone()
        } else {
            Default::default()
        };
        let acp = AcpManager::new(workspace.root(), &custom_agents);

        let credentials = CredentialStore::new();
        let mut notes = Vec::new();
        let ai = build_ai(&config, &credentials, &mut notes);

        let depth = ColorDepth::detect(config.ui.truecolor);
        let theme = Theme::from_name(ThemeName::parse(&config.ui.theme), depth);
        let layout = LayoutState::from_config(&config);
        let keymap = Keymap::for_preset(config.keymap.preset);
        let terminal = TerminalManager::new(config.terminal.scrollback);
        let git = GitRepository::discover(workspace.root());
        let mut output = VecDeque::new();
        output.push_back(format!("ttui v{}", crate::VERSION));
        output.extend(notes);

        Self {
            config,
            workspace,
            explorer,
            commands: CommandRegistry::builtin(),
            keymap,
            layout,
            theme,
            mode: EditMode::Insert,
            focus: FocusTarget::Editor,
            bottom_tab: BottomTab::Terminal,
            active_modal: None,
            status_message: None,
            should_quit: false,
            file_index: Vec::new(),
            fuzzy: FuzzyPicker::default(),
            terminal,
            git,
            lsp,
            acp,
            ai,
            credentials,
            output,
            search: BufferSearch::default(),
            register: String::new(),
            pending: None,
            command_buffer: String::new(),
            viewport: (120, 40),
            quit_armed: false,
            terminal_passthrough: false,
        }
    }

    pub fn active_index(&self) -> Option<usize> {
        self.workspace.active_index()
    }

    pub fn active_doc(&self) -> Option<&Document> {
        self.workspace.active()
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.workspace.active_mut()
    }

    pub fn set_status(&mut self, text: impl Into<String>, kind: MessageKind) {
        self.status_message = Some(StatusMessage::new(
            text,
            kind,
            Duration::from_millis(self.config.ui.status_message_ttl_ms),
        ));
    }

    pub fn push_output(&mut self, line: impl Into<String>) {
        if self.output.len() >= 2_000 {
            self.output.pop_front();
        }
        self.output.push_back(line.into());
    }

    pub fn cycle_theme(&mut self) {
        let next = self.theme.name.next();
        self.theme = Theme::from_name(next, self.theme.depth);
        self.set_status(format!("Theme: {}", next.label()), MessageKind::Info);
    }

    pub fn set_theme(&mut self, name: ThemeName) {
        self.theme = Theme::from_name(name, self.theme.depth);
    }

    /// Open a file and focus the editor.
    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        match self.workspace.open(path) {
            Ok(_) => {
                self.focus = FocusTarget::Editor;
                self.mode = EditMode::Insert;
                self.set_status(
                    format!("Opened {}", self.workspace.display_path(path)),
                    MessageKind::Info,
                );
                Ok(())
            }
            Err(error) => {
                self.set_status(format!("Open failed: {error}"), MessageKind::Error);
                Err(error.into())
            }
        }
    }

    /// Save the active buffer. Returns false for a pathless scratch buffer.
    pub fn save_active(&mut self) -> Result<bool> {
        let Some(document) = self.active_doc_mut() else {
            return Ok(false);
        };
        if document.path.is_none() {
            self.set_status(
                "Scratch buffer has no path (use :w <path>)",
                MessageKind::Warning,
            );
            return Ok(false);
        }
        match document.save() {
            Ok(()) => {
                let name = document.display_name();
                self.set_status(format!("Saved {name}"), MessageKind::Info);
                Ok(true)
            }
            Err(error) => {
                self.set_status(format!("Save failed: {error}"), MessageKind::Error);
                Err(error.into())
            }
        }
    }

    pub fn save_active_as(&mut self, path: &Path) -> Result<()> {
        if let Some(document) = self.active_doc_mut() {
            document.save_to(path)?;
            self.set_status(format!("Saved {}", path.display()), MessageKind::Info);
        }
        Ok(())
    }

    /// Keep the primary cursor inside the visible editor viewport.
    pub fn ensure_cursor_visible(&mut self) {
        let regions = compute(
            ratatui::layout::Rect::new(0, 0, self.viewport.0, self.viewport.1),
            &self.layout,
        );
        let height = regions.editor.height as usize;
        let offset = self.config.editor.scroll_off;
        if let Some(document) = self.active_doc_mut() {
            document.ensure_cursor_visible(height.max(1), offset);
        }
    }

    /// Row/column size available to a PTY in the bottom panel.
    pub fn terminal_size(&self) -> (u16, u16) {
        let regions = compute(
            ratatui::layout::Rect::new(0, 0, self.viewport.0, self.viewport.1),
            &self.layout,
        );
        match regions.bottom {
            Some(rect) => (
                rect.height.saturating_sub(3).max(2),
                rect.width.saturating_sub(2).max(2),
            ),
            None => (24, 80),
        }
    }

    pub fn spawn_terminal(&mut self) -> Result<()> {
        use crate::platform::shell::{detect_shell, fallback_shell};
        let shell = detect_shell(self.config.terminal.shell.as_deref());
        let (rows, cols) = self.terminal_size();
        let mut result = self
            .terminal
            .spawn(&shell, self.workspace.root(), rows, cols);
        if result.is_err() {
            let fallback = fallback_shell();
            result = self
                .terminal
                .spawn(&fallback, self.workspace.root(), rows, cols);
        }
        match result {
            Ok(_) => {
                self.bottom_tab = BottomTab::Terminal;
                self.layout.bottom_visible = true;
                self.focus = FocusTarget::Bottom(BottomTab::Terminal);
                self.terminal_passthrough = true;
                self.set_status(
                    "Terminal started — Ctrl+Space returns to panel navigation",
                    MessageKind::Info,
                );
                Ok(())
            }
            Err(error) => {
                self.set_status(format!("Terminal failed: {error}"), MessageKind::Error);
                Err(error.into())
            }
        }
    }

    /// Copy the primary selection into the register.
    pub fn yank_selection(&mut self) -> bool {
        let Some(document) = self.active_doc() else {
            return false;
        };
        let range = document.cursors.overall_range();
        if range.is_empty() {
            return false;
        }
        let text = document.rope.slice(range).to_string();
        if text.is_empty() {
            return false;
        }
        self.register = text;
        true
    }

    /// Copy the active line into the register.
    pub fn yank_line(&mut self) {
        if let Some(document) = self.active_doc() {
            let (line, _) = document.cursor_line_col();
            let text = document.line_text_at(line);
            self.register = text;
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let document = self.active_doc()?;
        let range = document.cursors.overall_range();
        if range.is_empty() {
            return None;
        }
        Some(document.rope.slice(range).to_string())
    }

    pub fn paste_register(&mut self) {
        if self.register.is_empty() {
            return;
        }
        let text = self.register.clone();
        if let Some(document) = self.active_doc_mut() {
            let _ = document.insert_text(&text, crate::editor::EditKind::Paste);
        }
        self.ensure_cursor_visible();
    }

    pub fn focus_cycle(&mut self, forward: bool) {
        let mut targets = vec![FocusTarget::Explorer, FocusTarget::Editor];
        if self.layout.bottom_visible {
            targets.push(FocusTarget::Bottom(self.bottom_tab));
        }
        if self.layout.right_visible {
            targets.push(FocusTarget::Ai);
        }
        let index = targets
            .iter()
            .position(|target| *target == self.focus)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % targets.len()
        } else {
            (index + targets.len() - 1) % targets.len()
        };
        self.focus = targets[next];
    }

    pub fn focus_explorer(&mut self) {
        self.layout.left_visible = true;
        self.focus = FocusTarget::Explorer;
    }

    pub fn focus_editor(&mut self) {
        self.focus = FocusTarget::Editor;
    }

    pub fn focus_bottom(&mut self, tab: BottomTab) {
        self.layout.bottom_visible = true;
        self.bottom_tab = tab;
        self.focus = FocusTarget::Bottom(tab);
        self.terminal_passthrough = tab == BottomTab::Terminal;
    }

    pub fn focus_ai(&mut self) {
        self.layout.right_visible = true;
        self.focus = FocusTarget::Ai;
    }

    /// Display name of the workspace root.
    pub fn workspace_name(&self) -> String {
        self.workspace
            .root()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string()
    }

    pub fn toggle_explorer(&mut self) {
        self.layout.toggle_explorer();
        if !self.layout.left_visible && self.focus == FocusTarget::Explorer {
            self.focus = FocusTarget::Editor;
        }
    }

    pub fn toggle_bottom(&mut self) {
        self.layout.toggle_bottom();
        if !self.layout.bottom_visible && matches!(self.focus, FocusTarget::Bottom(_)) {
            self.focus = FocusTarget::Editor;
        }
    }

    pub fn toggle_ai(&mut self) {
        self.layout.toggle_ai();
        if !self.layout.right_visible && self.focus == FocusTarget::Ai {
            self.focus = FocusTarget::Editor;
        }
    }

    pub fn toggle_zoom(&mut self) {
        self.layout.toggle_zoom();
        if self.layout.zoom {
            self.focus = FocusTarget::Editor;
        }
    }

    pub fn document_index(&self, path: &Path) -> Option<usize> {
        self.workspace.find_by_path(path)
    }

    #[cfg(test)]
    pub fn for_testing(root: &Path) -> Self {
        std::fs::create_dir_all(root).expect("test root");
        let config = Config::default();
        let workspace = Workspace::new(root).expect("workspace");
        let explorer = FileExplorer::new(root).expect("explorer");
        let mut state = Self::assemble(config, workspace, explorer);
        state
            .workspace
            .push_document(Document::from_text("fn main() {\n    let value = 42;\n}\n"));
        state
    }
}

fn build_ai(
    config: &Config,
    credentials: &CredentialStore,
    notes: &mut Vec<String>,
) -> AiCoordinator {
    let mut coordinator = AiCoordinator::new();
    let Some(provider_id) = config.ai.default_provider.clone() else {
        return coordinator;
    };
    let kind = ProviderKind::from_id(&provider_id).unwrap_or(ProviderKind::OpenAi);
    let settings = config
        .ai
        .providers
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    let endpoint = if settings.endpoint.trim().is_empty() {
        kind.default_endpoint().to_string()
    } else {
        settings.endpoint.clone()
    };
    let model = config
        .ai
        .model
        .clone()
        .or(settings.model.clone())
        .unwrap_or_else(|| default_model(kind).to_string());
    let env_var = settings
        .env_var
        .clone()
        .or_else(|| kind.default_env_var().map(str::to_string));
    let api_key = credentials.resolve(&provider_id, env_var.as_deref());

    if kind != ProviderKind::Ollama && api_key.is_none() {
        notes.push(format!(
            "AI provider '{provider_id}' has no API key; AI stays disabled."
        ));
        return coordinator;
    }

    let provider_config = ProviderConfig {
        id: provider_id.clone(),
        kind,
        endpoint,
        model: Some(model.clone()),
        api_key,
    };
    match provider_from_config(provider_config) {
        Ok(provider) => {
            let provider: Arc<dyn AiProvider> = Arc::from(provider);
            coordinator.configure(provider, model.clone());
            notes.push(format!("AI provider ready: {provider_id} ({model})"));
        }
        Err(error) => notes.push(format!("AI provider '{provider_id}' failed: {error}")),
    }
    coordinator
}

fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "gpt-4o-mini",
        ProviderKind::Anthropic => "claude-3-5-sonnet-latest",
        ProviderKind::Gemini => "gemini-2.0-flash",
        ProviderKind::Ollama => "qwen2.5-coder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_creates_a_scratch_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::bootstrap(dir.path(), None).expect("bootstrap");
        assert_eq!(state.workspace.len(), 1);
        assert_eq!(state.mode, EditMode::Insert);
        assert_eq!(state.focus, FocusTarget::Editor);
        assert!(!state.lsp.is_configured());
        assert_eq!(state.acp.state(), crate::acp::AcpState::Disconnected);
    }

    #[test]
    fn bootstrap_opens_requested_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write");
        let state = AppState::bootstrap(dir.path(), Some(&file)).expect("bootstrap");
        assert_eq!(state.workspace.len(), 1);
        assert_eq!(
            state.active_doc().map(Document::display_name).as_deref(),
            Some("main.rs")
        );
    }

    #[test]
    fn theme_cycle_wraps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        let start = state.theme.name;
        for _ in 0..ThemeName::ALL.len() {
            state.cycle_theme();
        }
        assert_eq!(state.theme.name, start);
    }

    #[test]
    fn focus_cycle_skips_hidden_panels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        state.layout.right_visible = false;
        state.layout.bottom_visible = false;
        state.focus = FocusTarget::Editor;
        state.focus_cycle(true);
        assert_eq!(state.focus, FocusTarget::Explorer);
        state.focus_cycle(true);
        assert_eq!(state.focus, FocusTarget::Editor);
    }

    #[test]
    fn terminal_size_is_sane_when_hidden() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        state.layout.bottom_visible = false;
        assert_eq!(state.terminal_size(), (24, 80));
    }
}
