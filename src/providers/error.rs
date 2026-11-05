//! BYOK provider errors.

use thiserror::Error;

/// Provider failure.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider {0} is not configured")]
    NotConfigured(String),

    #[error("missing API key for {0} (set the environment variable or run :ai set-key)")]
    MissingKey(String),

    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("provider returned HTTP {status}: {body}")]
    Status { status: u16, body: String },

    #[error("invalid provider response: {0}")]
    InvalidResponse(String),

    #[error("completion stream cancelled")]
    Cancelled,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("operation is not supported by this provider: {0}")]
    Unsupported(&'static str),
}
