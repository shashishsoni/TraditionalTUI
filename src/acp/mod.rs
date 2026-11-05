//! First-class Agent Client Protocol (ACP) integration.
//!
//! ACP is completely independent from BYOK providers. This subsystem owns
//! agent discovery, subprocess lifecycle, and the permission gatekeeper; the
//! UI renders the events and the app applies user decisions.

pub mod error;
pub mod gatekeeper;
pub mod permission;
pub mod protocol;
pub mod registry;
pub mod supervisor;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

pub use error::AcpError;
pub use gatekeeper::Gatekeeper;
pub use permission::{
    PermissionDecision, PermissionOption, PermissionOptionKind, PermissionPrompt,
};
pub use protocol::{AcpConnection, AcpEvent};
pub use registry::AcpAgentProfile;
pub use supervisor::AcpSupervisor;

use crate::config::AgentSettings;

/// Lifecycle state shown in the status bar and AI sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpState {
    Disconnected,
    Starting,
    Ready,
    Busy,
    Failed,
}

impl AcpState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Starting => "Starting",
            Self::Ready => "Ready",
            Self::Busy => "Working",
            Self::Failed => "Failed",
        }
    }
}

/// Owns agent discovery and at most one live agent connection.
pub struct AcpManager {
    profiles: Vec<AcpAgentProfile>,
    connection: Option<Arc<AcpConnection>>,
    agent: Option<String>,
    session_id: Option<String>,
    state: AcpState,
    gatekeeper: Gatekeeper,
    supervisor: AcpSupervisor,
    turn_tx: tokio::sync::mpsc::UnboundedSender<Result<(), String>>,
    turn_rx: tokio::sync::mpsc::UnboundedReceiver<Result<(), String>>,
    last_error: Option<String>,
}

impl std::fmt::Debug for AcpManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpManager")
            .field("agent", &self.agent)
            .field("session_id", &self.session_id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl AcpManager {
    pub fn new(root: impl AsRef<Path>, custom_agents: &BTreeMap<String, AgentSettings>) -> Self {
        let (turn_tx, turn_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            profiles: registry::discover(custom_agents),
            connection: None,
            agent: None,
            session_id: None,
            state: AcpState::Disconnected,
            gatekeeper: Gatekeeper::new(root),
            supervisor: AcpSupervisor::default(),
            turn_tx,
            turn_rx,
            last_error: None,
        }
    }

    pub fn profiles(&self) -> &[AcpAgentProfile] {
        &self.profiles
    }

    pub fn available_agents(&self) -> Vec<&AcpAgentProfile> {
        registry::available_profiles(&self.profiles)
    }

    pub fn state(&self) -> AcpState {
        self.state
    }

    pub fn agent_name(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn gatekeeper(&self) -> &Gatekeeper {
        &self.gatekeeper
    }

    pub fn supervisor(&self) -> &AcpSupervisor {
        &self.supervisor
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    /// Spawn and initialize an agent. Custom agents require workspace trust.
    pub async fn start(&mut self, agent_id: &str, trusted: bool) -> Result<(), AcpError> {
        let profile = registry::find(&self.profiles, agent_id)
            .cloned()
            .ok_or_else(|| AcpError::AgentUnavailable(agent_id.to_string()))?;
        if !profile.available {
            return Err(AcpError::AgentUnavailable(profile.invocation()));
        }
        if profile.custom && !trusted {
            return Err(AcpError::UntrustedWorkspace);
        }

        self.state = AcpState::Starting;
        self.supervisor
            .record(format!("spawning {}", profile.invocation()));
        let connection =
            AcpConnection::spawn(&profile.command, &profile.args, self.gatekeeper.root()).await?;
        let info = connection.initialize().await?;
        let version = info
            .get("protocolVersion")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        self.supervisor
            .record(format!("initialized {} (ACP v{version})", profile.name));

        self.connection = Some(Arc::new(connection));
        self.agent = Some(profile.name);
        self.session_id = None;
        self.state = AcpState::Ready;
        self.last_error = None;
        Ok(())
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Create a session for the connected agent.
    pub async fn new_session(&mut self) -> Result<String, AcpError> {
        let root = self.gatekeeper.root().to_path_buf();
        let connection = self.connection.clone().ok_or(AcpError::NotRunning)?;
        let session_id = connection.new_session(&root).await?;
        self.session_id = Some(session_id.clone());
        self.supervisor
            .record(format!("session created: {session_id}"));
        Ok(session_id)
    }

    /// Send a prompt turn without blocking the render loop.
    pub fn send_prompt(&mut self, text: &str) -> Result<(), AcpError> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| AcpError::Protocol("no active ACP session".into()))?;
        let connection = self.connection.clone().ok_or(AcpError::NotRunning)?;
        let text = text.to_string();
        let turn_tx = self.turn_tx.clone();
        self.state = AcpState::Busy;
        self.last_error = None;
        self.supervisor.record("prompt sent");
        tokio::spawn(async move {
            let result = connection
                .prompt(&session_id, &text)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string());
            let _ = turn_tx.send(result);
        });
        Ok(())
    }

    /// Cancel the active turn without blocking.
    pub fn cancel(&mut self) -> Result<(), AcpError> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| AcpError::Protocol("no active ACP session".into()))?;
        let connection = self.connection.clone().ok_or(AcpError::NotRunning)?;
        let turn_tx = self.turn_tx.clone();
        self.supervisor.record("turn cancelled");
        tokio::spawn(async move {
            let result = connection
                .cancel(&session_id)
                .await
                .map_err(|error| error.to_string());
            let _ = turn_tx.send(result);
        });
        Ok(())
    }

    /// Drain agent events, updating connection and turn state.
    pub fn poll_events(&mut self) -> Vec<AcpEvent> {
        let events = self
            .connection
            .as_ref()
            .map(|connection| connection.poll_events())
            .unwrap_or_default();
        for event in &events {
            match event {
                AcpEvent::Notification { method, .. } => {
                    self.supervisor.record(format!("notification: {method}"));
                }
                AcpEvent::Request { method, .. } => {
                    self.supervisor.record(format!("agent request: {method}"));
                }
                AcpEvent::Stderr(line) => self.supervisor.record(line.clone()),
                AcpEvent::Exited(status) => {
                    self.supervisor.record(match status {
                        Some(status) => format!("agent exited: {status}"),
                        None => "agent exited".to_string(),
                    });
                    self.state = AcpState::Disconnected;
                    self.session_id = None;
                }
            }
        }

        while let Ok(result) = self.turn_rx.try_recv() {
            match result {
                Ok(()) => {
                    self.state = AcpState::Ready;
                    self.supervisor.record("turn finished");
                }
                Err(error) => {
                    self.state = AcpState::Failed;
                    self.supervisor.record(format!("turn failed: {error}"));
                    self.last_error = Some(error);
                }
            }
        }
        events
    }

    pub fn has_pending_events(&self) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|connection| connection.has_pending_events())
            || !self.turn_rx.is_empty()
    }

    pub async fn respond(&mut self, id: Value, result: Value) -> Result<(), AcpError> {
        let connection = self.connection.clone().ok_or(AcpError::NotRunning)?;
        connection.respond(id, result).await
    }

    pub async fn respond_error(
        &mut self,
        id: Value,
        code: i64,
        message: &str,
    ) -> Result<(), AcpError> {
        let connection = self.connection.clone().ok_or(AcpError::NotRunning)?;
        connection.respond_error(id, code, message).await
    }

    /// Resolve a permission card with the matching agent-provided option.
    pub async fn respond_permission(
        &mut self,
        prompt: &PermissionPrompt,
        decision: PermissionDecision,
    ) -> Result<(), AcpError> {
        match prompt.select(decision) {
            Some(option) => {
                let result = json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option.option_id,
                    }
                });
                self.supervisor
                    .record(format!("permission {:?} -> {}", decision, option.option_id));
                self.respond(prompt.id.clone(), result).await
            }
            None => {
                self.supervisor.record("permission cancelled");
                self.respond(
                    prompt.id.clone(),
                    json!({ "outcome": { "outcome": "cancelled" } }),
                )
                .await
            }
        }
    }

    /// Respond to `fs/read_text_file` with the live buffer contents.
    pub async fn respond_fs_read(&mut self, id: Value, content: &str) -> Result<(), AcpError> {
        self.respond(id, json!({ "content": content })).await
    }

    /// Acknowledge a successful `fs/write_text_file`.
    pub async fn respond_fs_write(&mut self, id: Value) -> Result<(), AcpError> {
        self.respond(id, Value::Null).await
    }

    pub async fn shutdown(&mut self) {
        if let Some(connection) = self.connection.take() {
            connection.shutdown().await;
        }
        self.state = AcpState::Disconnected;
        self.session_id = None;
        self.supervisor.record("agent shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_starts_disconnected_with_profiles() {
        let manager = AcpManager::new("/tmp", &BTreeMap::new());
        assert_eq!(manager.state(), AcpState::Disconnected);
        assert!(!manager.profiles().is_empty());
        assert!(manager.agent_name().is_none());
    }

    #[tokio::test]
    async fn unavailable_agent_is_rejected() {
        let mut custom = BTreeMap::new();
        custom.insert(
            "ghost".to_string(),
            AgentSettings {
                command: "definitely-not-installed-ttui-agent".into(),
                ..Default::default()
            },
        );
        let mut manager = AcpManager::new("/tmp", &custom);
        let error = manager.start("ghost", true).await.expect_err("must fail");
        assert!(matches!(error, AcpError::AgentUnavailable(_)));
        assert_eq!(manager.state(), AcpState::Disconnected);
    }

    #[tokio::test]
    async fn custom_agent_requires_trust() {
        let mut custom = BTreeMap::new();
        custom.insert(
            "shell-agent".to_string(),
            AgentSettings {
                command: "sh".into(),
                ..Default::default()
            },
        );
        let mut manager = AcpManager::new("/tmp", &custom);
        let error = manager
            .start("shell-agent", false)
            .await
            .expect_err("must fail");
        assert!(matches!(error, AcpError::UntrustedWorkspace));
    }

    #[tokio::test]
    async fn shutdown_without_connection_is_safe() {
        let mut manager = AcpManager::new("/tmp", &BTreeMap::new());
        manager.shutdown().await;
        assert_eq!(manager.state(), AcpState::Disconnected);
    }
}
