//! Hierarchical TOML configuration.
//!
//! Resolution order (later sources override earlier ones):
//! `built-in defaults` -> `global user config` -> `workspace .ttui/config.toml`.
//!
//! Nested tables are deep-merged so a workspace only needs to declare the
//! fields it overrides.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod loader;

pub use loader::{ConfigError, load, load_from, workspace_config_path};

/// Default global theme name.
pub const DEFAULT_THEME: &str = "tokyonight";

/// Starter configuration written by `:config` when no user config exists.
pub const CONFIG_TEMPLATE: &str = r#"# ttui configuration. Every section is optional.
# Global file: ~/.config/ttui/config.toml
# Workspace file: <project>/.ttui/config.toml

[editor]
tab_width = 4
expand_tab = true
line_numbers = true
relative_line_numbers = false
scroll_off = 4
auto_save = false
format_on_save = false

[ui]
theme = "tokyonight" # tokyonight | catppuccin-mocha | gruvbox-dark | one-dark-pro | catppuccin-latte
left_width = 28
right_width = 38
bottom_height = 12
show_left_sidebar = true
show_right_sidebar = true
show_bottom_panel = true
mouse = true
# truecolor = true

[terminal]
# shell = "/bin/zsh"
scrollback = 10000

[ai]
enabled = false
# default_provider = "openai"
# model = "gpt-4o-mini"

# [ai.providers.openai]
# endpoint = "https://api.openai.com/v1"
# env_var = "OPENAI_API_KEY"

[lsp]
enabled = true

# [lsp.servers.rust-analyzer]
# command = "rust-analyzer"
# languages = ["rust"]

[acp]
enabled = true
trusted_only = true

# [acp.agents.my-agent]
# command = "my-agent"
# args = ["acp"]
"#;

/// Keymap preset selected by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeymapPreset {
    /// Vim / Helix modal editing.
    #[default]
    Vim,
    /// VS Code style editing.
    Vscode,
}

/// Root configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub editor: EditorConfig,
    pub ui: UiConfig,
    pub terminal: TerminalConfig,
    pub lsp: LspConfig,
    pub ai: AiConfig,
    pub acp: AcpConfig,
    pub performance: PerformanceConfig,
    pub keymap: KeymapConfig,
}

impl Config {
    /// Clamp values into their documented ranges and drop invalid entries.
    pub fn normalize(&mut self) {
        self.ui.left_width = self.ui.left_width.clamp(18, 50);
        self.ui.right_width = self.ui.right_width.clamp(26, 70);
        self.ui.bottom_height = self.ui.bottom_height.clamp(6, 30);
        self.editor.tab_width = self.editor.tab_width.clamp(1, 16);
        self.terminal.scrollback = self.terminal.scrollback.clamp(100, 100_000);
        self.performance.large_file_threshold_mb = self.performance.large_file_threshold_mb.max(1);
        self.performance.max_project_search_results =
            self.performance.max_project_search_results.clamp(1, 20_000);
        self.performance.max_file_index_entries =
            self.performance.max_file_index_entries.max(1_000);
        if self.ui.theme.trim().is_empty() {
            self.ui.theme = DEFAULT_THEME.to_string();
        }
    }
}

/// Editor behaviour settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub tab_width: usize,
    pub expand_tab: bool,
    pub line_numbers: bool,
    pub relative_line_numbers: bool,
    pub scroll_off: usize,
    pub auto_save: bool,
    pub format_on_save: bool,
    pub large_file_threshold_mb: u64,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_width: 4,
            expand_tab: true,
            line_numbers: true,
            relative_line_numbers: false,
            scroll_off: 4,
            auto_save: false,
            format_on_save: false,
            large_file_threshold_mb: 10,
        }
    }
}

/// Workbench layout and appearance settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme: String,
    pub left_width: u16,
    pub right_width: u16,
    pub bottom_height: u16,
    pub show_left_sidebar: bool,
    pub show_right_sidebar: bool,
    pub show_bottom_panel: bool,
    pub mouse: bool,
    /// `None` = auto-detect from `COLORTERM`.
    pub truecolor: Option<bool>,
    pub status_message_ttl_ms: u64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            left_width: 28,
            right_width: 38,
            bottom_height: 12,
            show_left_sidebar: true,
            show_right_sidebar: true,
            show_bottom_panel: true,
            mouse: true,
            truecolor: None,
            status_message_ttl_ms: 4_000,
        }
    }
}

/// Integrated terminal settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub shell: Option<String>,
    pub scrollback: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: None,
            scrollback: 10_000,
        }
    }
}

/// Language server settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LspConfig {
    pub enabled: bool,
    pub servers: BTreeMap<String, LspServerSettings>,
}

/// One language server command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LspServerSettings {
    pub command: String,
    pub args: Vec<String>,
    pub languages: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// BYOK AI settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AiConfig {
    pub enabled: bool,
    pub default_provider: Option<String>,
    pub model: Option<String>,
    pub providers: BTreeMap<String, ProviderSettings>,
}

/// One BYOK provider entry. API keys never live here; see [`crate::providers::credentials`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderSettings {
    pub endpoint: String,
    pub model: Option<String>,
    pub env_var: Option<String>,
}

/// ACP agent settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AcpConfig {
    pub enabled: bool,
    pub trusted_only: bool,
    pub agents: BTreeMap<String, AgentSettings>,
}

/// One configured ACP agent command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentSettings {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub adapter: bool,
}

/// Performance guardrails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    pub large_file_threshold_mb: u64,
    pub max_project_search_results: usize,
    pub max_file_index_entries: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            large_file_threshold_mb: 10,
            max_project_search_results: 500,
            max_file_index_entries: 200_000,
        }
    }
}

/// Keymap settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeymapConfig {
    pub preset: KeymapPreset,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_consistent() {
        let config = Config::default();
        assert_eq!(config.ui.theme, DEFAULT_THEME);
        assert_eq!(config.editor.tab_width, 4);
        assert_eq!(config.keymap.preset, KeymapPreset::Vim);
    }

    #[test]
    fn normalize_clamps_out_of_range_values() {
        let mut config = Config::default();
        config.ui.left_width = 2;
        config.ui.right_width = 400;
        config.ui.bottom_height = 1;
        config.editor.tab_width = 0;
        config.ui.theme = "   ".into();
        config.normalize();
        assert_eq!(config.ui.left_width, 18);
        assert_eq!(config.ui.right_width, 70);
        assert_eq!(config.ui.bottom_height, 6);
        assert_eq!(config.editor.tab_width, 1);
        assert_eq!(config.ui.theme, DEFAULT_THEME);
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let config: Config = toml::from_str("[editor]\ntab_width = 2\n").expect("parse");
        assert_eq!(config.editor.tab_width, 2);
        assert_eq!(config.ui.left_width, 28);
    }
}
