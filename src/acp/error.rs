//! ACP domain errors.

use std::path::PathBuf;

use thiserror::Error;

/// Failure while supervising an ACP agent.
#[derive(Debug, Error)]
pub enum AcpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to spawn ACP agent {agent}: {source}")]
    Spawn {
        agent: String,
        #[source]
        source: std::io::Error,
    },

    #[error("ACP protocol error: {0}")]
    Protocol(String),

    #[error("ACP request timed out")]
    Timeout,

    #[error("ACP connection is not running")]
    NotRunning,

    #[error("the selected agent is not installed: {0}")]
    AgentUnavailable(String),

    #[error("workspace must be trusted before running custom agent commands")]
    UntrustedWorkspace,

    #[error("path is outside the workspace: {0}")]
    OutsideWorkspace(PathBuf),

    #[error("path must be absolute: {0}")]
    RelativePath(PathBuf),

    #[error("access to sensitive path requires confirmation: {0}")]
    SensitivePath(PathBuf),
}
