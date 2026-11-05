//! Capability-gated plugin registry.
//!
//! Phase 1–4 plugins are declarative TOML manifests: themes, keymaps, task
//! runners and language definitions. Capabilities are explicit and must be
//! user-approved before a plugin may execute commands or touch the network.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Capabilities a plugin may request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    ReadFiles,
    WriteFiles,
    ExecuteCommands,
    Network,
    UserInterface,
}

impl PluginCapability {
    /// Capabilities that require explicit user consent before use.
    pub fn requires_consent(self) -> bool {
        matches!(
            self,
            Self::WriteFiles | Self::ExecuteCommands | Self::Network
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadFiles => "read files",
            Self::WriteFiles => "write files",
            Self::ExecuteCommands => "execute commands",
            Self::Network => "network",
            Self::UserInterface => "user interface",
        }
    }
}

/// Declarative task contributed by a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PluginTask {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Parsed plugin manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: BTreeSet<PluginCapability>,
    pub tasks: Vec<PluginTask>,
}

impl PluginManifest {
    pub fn parse(text: &str) -> Result<Self, PluginError> {
        let manifest: Self = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), PluginError> {
        if self.name.trim().is_empty() {
            return Err(PluginError::InvalidManifest(
                "plugin name must not be empty".into(),
            ));
        }
        for task in &self.tasks {
            if task.name.trim().is_empty() || task.command.trim().is_empty() {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin {} has a task without name or command",
                    self.name
                )));
            }
        }
        Ok(())
    }

    pub fn has_capability(&self, capability: PluginCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// Plugin loading failure.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid plugin TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),

    #[error("plugin {0} is already registered")]
    Duplicate(String),
}

/// Registry of loaded plugin manifests.
#[derive(Debug, Default)]
pub struct PluginManager {
    manifests: Vec<PluginManifest>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn manifests(&self) -> &[PluginManifest] {
        &self.manifests
    }

    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Register a manifest, rejecting duplicate plugin names.
    pub fn register(&mut self, manifest: PluginManifest) -> Result<(), PluginError> {
        manifest.validate()?;
        if self
            .manifests
            .iter()
            .any(|existing| existing.name == manifest.name)
        {
            return Err(PluginError::Duplicate(manifest.name));
        }
        self.manifests.push(manifest);
        Ok(())
    }

    /// Load every `*.toml` manifest in a directory.
    pub fn load_from_dir(&mut self, dir: &Path) -> Result<usize, PluginError> {
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut loaded = 0;
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect();
        entries.sort();
        for path in entries {
            let text = std::fs::read_to_string(&path)?;
            self.register(PluginManifest::parse(&text)?)?;
            loaded += 1;
        }
        Ok(loaded)
    }

    pub fn find(&self, name: &str) -> Option<&PluginManifest> {
        self.manifests.iter().find(|manifest| manifest.name == name)
    }

    /// Task lookup across all plugins.
    pub fn find_task(&self, task_name: &str) -> Option<(&PluginManifest, &PluginTask)> {
        self.manifests.iter().find_map(|manifest| {
            manifest
                .tasks
                .iter()
                .find(|task| task.name == task_name)
                .map(|task| (manifest, task))
        })
    }

    /// Whether a plugin may perform `capability` without a fresh prompt.
    pub fn allows(&self, plugin: &str, capability: PluginCapability) -> bool {
        self.find(plugin)
            .is_some_and(|manifest| manifest.has_capability(capability))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_manifest() {
        let manifest = PluginManifest::parse(
            r#"
            name = "rust-tools"
            version = "0.1.0"
            description = "Rust helpers"
            capabilities = ["read_files", "execute_commands"]

            [[tasks]]
            name = "test"
            command = "cargo"
            args = ["test"]
            "#,
        )
        .expect("parse");
        assert!(manifest.has_capability(PluginCapability::ExecuteCommands));
        assert_eq!(manifest.tasks.len(), 1);
    }

    #[test]
    fn empty_name_is_rejected() {
        let error = PluginManifest::parse("version = \"1\"").expect_err("must fail");
        assert!(matches!(error, PluginError::InvalidManifest(_)));
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let mut manager = PluginManager::new();
        let manifest = PluginManifest {
            name: "p".into(),
            ..Default::default()
        };
        manager.register(manifest.clone()).expect("first");
        assert!(matches!(
            manager.register(manifest),
            Err(PluginError::Duplicate(_))
        ));
    }

    #[test]
    fn loads_directory_of_manifests() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.toml"), "name = \"a\"\n").expect("write");
        std::fs::write(dir.path().join("b.toml"), "name = \"b\"\n").expect("write");
        std::fs::write(dir.path().join("ignore.txt"), "name = \"c\"\n").expect("write");
        let mut manager = PluginManager::new();
        assert_eq!(manager.load_from_dir(dir.path()).expect("load"), 2);
    }

    #[test]
    fn consent_requirement_is_capability_specific() {
        assert!(!PluginCapability::ReadFiles.requires_consent());
        assert!(PluginCapability::ExecuteCommands.requires_consent());
    }
}
