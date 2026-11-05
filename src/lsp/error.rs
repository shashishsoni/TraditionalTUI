//! LSP domain errors.

use thiserror::Error;

/// Failure while supervising a language server.
#[derive(Debug, Error)]
pub enum LspError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to spawn {server}: {source}")]
    Spawn {
        server: String,
        #[source]
        source: std::io::Error,
    },

    #[error("language server protocol error: {0}")]
    Protocol(String),

    #[error("language server request timed out")]
    Timeout,

    #[error("language server connection closed")]
    Closed,

    #[error("invalid path for URI conversion: {0}")]
    InvalidPath(String),
}
