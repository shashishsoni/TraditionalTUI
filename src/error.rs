//! Top-level error boundary for the application.
//!
//! Domain modules expose their own typed [`thiserror`] enums. This module
//! unifies them for the `main` binary, which uses `anyhow` only at the very
//! outer boundary while keeping runtime paths typed.

use thiserror::Error;

/// Unified application error.
#[derive(Debug, Error)]
pub enum TtuiError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("editor error: {0}")]
    Editor(#[from] crate::editor::EditorError),

    #[error("workspace error: {0}")]
    Workspace(#[from] crate::workspace::WorkspaceError),

    #[error("explorer error: {0}")]
    Explorer(#[from] crate::explorer::ExplorerError),

    #[error("terminal error: {0}")]
    Terminal(#[from] crate::terminal::TerminalError),

    #[error("LSP error: {0}")]
    Lsp(#[from] crate::lsp::LspError),

    #[error("ACP error: {0}")]
    Acp(#[from] crate::acp::AcpError),

    #[error("provider error: {0}")]
    Provider(#[from] crate::providers::ProviderError),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("feature not implemented yet: {0}")]
    NotImplemented(&'static str),
}

/// Convenience result alias used across the crate.
pub type Result<T> = std::result::Result<T, TtuiError>;
