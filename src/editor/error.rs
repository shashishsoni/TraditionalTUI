//! Editor domain errors.

use thiserror::Error;

/// Errors produced by the rope/document engine.
#[derive(Debug, Error)]
pub enum EditorError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("file is not valid UTF-8")]
    InvalidUtf8,

    #[error("file contains invalid UTF-16 data")]
    InvalidUtf16,

    #[error("buffer has no filesystem path")]
    NoPath,

    #[error("invalid char range {start}..{end} for buffer of {len} chars")]
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },

    #[error("offset {offset} is outside the buffer ({len} chars)")]
    OutOfBounds { offset: usize, len: usize },
}

impl EditorError {
    pub(crate) fn range(start: usize, end: usize, len: usize) -> Self {
        Self::InvalidRange { start, end, len }
    }
}
