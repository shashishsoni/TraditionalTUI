//! Command registry and keybinding resolver.
//!
//! Commands are declared as static specs. The app maps a resolved command id
//! to an [`crate::app::Action`]; keeping the registry presentation-free lets the
//! command palette, status bar hints and keymap share one source of truth.

mod keymap;

pub use keymap::Keymap;

/// Stable command identifiers.
pub mod ids {
    pub const SAVE: &str = "file.save";
    pub const SAVE_ALL: &str = "file.save_all";
    pub const CLOSE_TAB: &str = "file.close_tab";
    pub const QUIT: &str = "app.quit";
    pub const HELP: &str = "app.help";
    pub const TOGGLE_EXPLORER: &str = "view.toggle_explorer";
    pub const TOGGLE_BOTTOM: &str = "view.toggle_bottom";
    pub const TOGGLE_AI: &str = "view.toggle_ai";
    pub const ZOOM: &str = "view.zoom";
    pub const FILE_FINDER: &str = "finder.file";
    pub const COMMAND_PALETTE: &str = "palette.command";
    pub const PROJECT_SEARCH: &str = "search.project";
    pub const BUFFER_SEARCH: &str = "search.buffer";
    pub const UNDO: &str = "edit.undo";
    pub const REDO: &str = "edit.redo";
    pub const SELECT_ALL: &str = "edit.select_all";
    pub const DELETE_LINE: &str = "edit.delete_line";
    pub const COPY: &str = "edit.copy";
    pub const PASTE: &str = "edit.paste";
    pub const INSERT_MODE: &str = "editor.insert_mode";
    pub const NORMAL_MODE: &str = "editor.normal_mode";
    pub const NEXT_THEME: &str = "theme.next";
    pub const GRANT_TRUST: &str = "workspace.trust";
    pub const REFRESH_GIT: &str = "git.refresh";
    pub const NEW_TERMINAL: &str = "terminal.new";
    pub const CLOSE_TERMINAL: &str = "terminal.close";
    pub const FOCUS_AI: &str = "ai.focus";
    pub const RELOAD_FILE: &str = "file.reload";
    pub const CONNECT_AGENT: &str = "acp.connect";
    pub const OPEN_CONFIG: &str = "config.open";
}

/// Declarative command description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub binding: Option<&'static str>,
}

/// Static registry of built-in commands.
#[derive(Debug, Clone)]
pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl CommandRegistry {
    pub fn builtin() -> Self {
        use ids::*;
        let specs = vec![
            spec(SAVE, "Save File", "File", Some("ctrl-s")),
            spec(SAVE_ALL, "Save All Files", "File", None),
            spec(RELOAD_FILE, "Reload File From Disk", "File", None),
            spec(CLOSE_TAB, "Close Active Tab", "File", Some("ctrl-w c")),
            spec(QUIT, "Quit", "Application", Some("ctrl-q")),
            spec(HELP, "Show Keymap Help", "Application", Some("f1")),
            spec(TOGGLE_EXPLORER, "Toggle Explorer", "View", Some("ctrl-b")),
            spec(TOGGLE_BOTTOM, "Toggle Bottom Panel", "View", Some("ctrl-j")),
            spec(TOGGLE_AI, "Toggle AI Sidebar", "View", Some("ctrl-alt-b")),
            spec(ZOOM, "Toggle Zen Mode", "View", Some("ctrl-w z")),
            spec(FILE_FINDER, "Go to File", "Navigation", Some("ctrl-p")),
            spec(
                COMMAND_PALETTE,
                "Command Palette",
                "Navigation",
                Some("ctrl-shift-p"),
            ),
            spec(
                PROJECT_SEARCH,
                "Search in Project",
                "Search",
                Some("ctrl-shift-f"),
            ),
            spec(BUFFER_SEARCH, "Search in Buffer", "Search", Some("ctrl-f")),
            spec(UNDO, "Undo", "Edit", Some("ctrl-z")),
            spec(REDO, "Redo", "Edit", Some("ctrl-y")),
            spec(SELECT_ALL, "Select All", "Edit", Some("ctrl-a")),
            spec(DELETE_LINE, "Delete Line", "Edit", None),
            spec(COPY, "Copy Selection", "Edit", Some("ctrl-c")),
            spec(PASTE, "Paste Clipboard", "Edit", Some("ctrl-v")),
            spec(INSERT_MODE, "Enter Insert Mode", "Editor", None),
            spec(NORMAL_MODE, "Enter Normal Mode", "Editor", Some("escape")),
            spec(NEXT_THEME, "Cycle Theme", "Theme", None),
            spec(GRANT_TRUST, "Trust Workspace", "Workspace", None),
            spec(REFRESH_GIT, "Refresh Git Status", "Git", None),
            spec(NEW_TERMINAL, "New Terminal", "Terminal", Some("ctrl-t")),
            spec(CLOSE_TERMINAL, "Close Terminal", "Terminal", None),
            spec(FOCUS_AI, "Focus AI Assistant", "AI", None),
            spec(CONNECT_AGENT, "Connect ACP Agent", "AI", Some("ctrl-g")),
            spec(
                OPEN_CONFIG,
                "Open Configuration",
                "Application",
                Some("ctrl-o"),
            ),
        ];
        Self { specs }
    }

    pub fn all(&self) -> &[CommandSpec] {
        &self.specs
    }

    pub fn find(&self, id: &str) -> Option<&CommandSpec> {
        self.specs.iter().find(|spec| spec.id == id)
    }

    /// Case-insensitive palette filter over id, title and category.
    pub fn filter(&self, query: &str) -> Vec<&CommandSpec> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.specs.iter().collect();
        }
        self.specs
            .iter()
            .filter(|spec| {
                spec.title.to_ascii_lowercase().contains(&needle)
                    || spec.id.to_ascii_lowercase().contains(&needle)
                    || spec.category.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn register(&mut self, spec: CommandSpec) {
        if self.find(spec.id).is_none() {
            self.specs.push(spec);
        }
    }
}

const fn spec(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    binding: Option<&'static str>,
) -> CommandSpec {
    CommandSpec {
        id,
        title,
        category,
        binding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_filter_matches_title_and_category() {
        let registry = CommandRegistry::builtin();
        assert!(
            registry
                .filter("save")
                .iter()
                .any(|spec| spec.id == ids::SAVE)
        );
        assert!(
            registry
                .filter("terminal")
                .iter()
                .any(|spec| spec.id == ids::NEW_TERMINAL)
        );
        assert_eq!(registry.filter("save").len(), 2);
    }

    #[test]
    fn duplicate_registration_is_ignored() {
        let mut registry = CommandRegistry::builtin();
        let count = registry.all().len();
        registry.register(CommandSpec {
            id: ids::SAVE,
            title: "Duplicate",
            category: "Test",
            binding: None,
        });
        assert_eq!(registry.all().len(), count);
    }
}
