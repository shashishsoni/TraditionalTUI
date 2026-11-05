//! Multiplexed Language Server Protocol client manager.
//!
//! Servers are spawned lazily on first use and never during cold startup. When
//! no servers are configured the manager is inert and imposes zero background
//! work on the IDE.

pub mod client;
pub mod diagnostics;
pub mod error;
pub mod position;
pub mod transport;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use tokio::sync::mpsc;

pub use client::{LspClient, LspEvent, LspServerConfig, ServerState, SyncKind};
pub use diagnostics::{DiagnosticEntry, DiagnosticStore, Severity};
pub use error::LspError;
pub use position::{PositionEncoding, from_lsp_position, to_lsp_position};

use crate::config::LspServerSettings;

/// Owns every configured language server connection.
pub struct LspManager {
    configs: Vec<LspServerConfig>,
    clients: HashMap<String, LspClient>,
    events_tx: mpsc::UnboundedSender<LspEvent>,
    events_rx: mpsc::UnboundedReceiver<LspEvent>,
    diagnostics: DiagnosticStore,
}

impl std::fmt::Debug for LspManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspManager")
            .field(
                "configs",
                &self.configs.iter().map(|c| &c.name).collect::<Vec<_>>(),
            )
            .field("running", &self.clients.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl LspManager {
    pub fn new(configs: Vec<LspServerConfig>) -> Self {
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            configs,
            clients: HashMap::new(),
            events_tx,
            events_rx,
            diagnostics: DiagnosticStore::default(),
        }
    }

    /// Build from `[lsp.servers.*]` configuration entries.
    pub fn from_settings(settings: &BTreeMap<String, LspServerSettings>) -> Self {
        let configs = settings
            .iter()
            .filter(|(_, server)| !server.command.trim().is_empty())
            .map(|(name, server)| LspServerConfig {
                name: name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                languages: server.languages.clone(),
                env: server.env.clone(),
            })
            .collect();
        Self::new(configs)
    }

    pub fn is_configured(&self) -> bool {
        !self.configs.is_empty()
    }

    pub fn configs(&self) -> &[LspServerConfig] {
        &self.configs
    }

    pub fn running_servers(&self) -> Vec<&str> {
        self.clients.values().map(LspClient::name).collect()
    }

    pub fn server_for_language(&self, language: &str) -> Option<&LspServerConfig> {
        self.configs.iter().find(|config| config.supports(language))
    }

    /// Spawn and initialize the server for `language` if needed.
    pub async fn ensure_server(
        &mut self,
        language: &str,
        root: &Path,
    ) -> Result<Option<String>, LspError> {
        let Some(config) = self.server_for_language(language).cloned() else {
            return Ok(None);
        };
        if self.clients.contains_key(&config.name) {
            return Ok(Some(config.name));
        }
        let mut client = LspClient::spawn(&config, root, self.events_tx.clone()).await?;
        client.initialize(root).await?;
        let name = config.name.clone();
        self.clients.insert(name.clone(), client);
        Ok(Some(name))
    }

    pub async fn did_open(
        &mut self,
        language: &str,
        root: &Path,
        path: &Path,
        version: u64,
        text: &str,
    ) -> Result<(), LspError> {
        let Some(name) = self.ensure_server(language, root).await? else {
            return Ok(());
        };
        if let Some(client) = self.clients.get(&name) {
            client.did_open(path, language, version, text).await?;
        }
        Ok(())
    }

    pub async fn did_change(
        &mut self,
        language: &str,
        root: &Path,
        path: &Path,
        version: u64,
        text: &str,
    ) -> Result<(), LspError> {
        let Some(name) = self.ensure_server(language, root).await? else {
            return Ok(());
        };
        if let Some(client) = self.clients.get(&name) {
            client.did_change(path, version, text).await?;
        }
        Ok(())
    }

    pub async fn did_save(
        &mut self,
        language: &str,
        root: &Path,
        path: &Path,
        text: Option<&str>,
    ) -> Result<(), LspError> {
        let Some(name) = self.ensure_server(language, root).await? else {
            return Ok(());
        };
        if let Some(client) = self.clients.get(&name) {
            client.did_save(path, text).await?;
        }
        Ok(())
    }

    pub async fn did_close(&mut self, language: &str, path: &Path) -> Result<(), LspError> {
        if let Some(config) = self.server_for_language(language) {
            if let Some(client) = self.clients.get(&config.name) {
                client.did_close(path).await?;
            }
        }
        Ok(())
    }

    /// Drain background events and update the diagnostics store.
    pub fn poll_events(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            if let LspEvent::Diagnostics { uri, entries } = &event {
                self.diagnostics.set(uri.clone(), entries.clone());
            }
            events.push(event);
        }
        events
    }

    pub fn has_pending_events(&self) -> bool {
        !self.events_rx.is_empty()
    }

    pub fn diagnostics(&self) -> &DiagnosticStore {
        &self.diagnostics
    }

    pub async fn shutdown_all(&mut self) {
        for (_, mut client) in self.clients.drain() {
            client.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manager_is_inert() {
        let manager = LspManager::new(Vec::new());
        assert!(!manager.is_configured());
        assert!(manager.server_for_language("rust").is_none());
    }

    #[test]
    fn settings_become_server_configs() {
        let mut settings = BTreeMap::new();
        settings.insert(
            "rust-analyzer".to_string(),
            LspServerSettings {
                command: "rust-analyzer".into(),
                languages: vec!["rust".into()],
                ..Default::default()
            },
        );
        settings.insert("disabled".to_string(), LspServerSettings::default());
        let manager = LspManager::from_settings(&settings);
        assert_eq!(manager.configs().len(), 1);
        assert!(manager.server_for_language("rust").is_some());
    }

    #[tokio::test]
    async fn unconfigured_language_is_a_noop() {
        let mut manager = LspManager::new(Vec::new());
        let result = manager
            .ensure_server("rust", Path::new("/tmp"))
            .await
            .expect("no-op");
        assert!(result.is_none());
    }
}
