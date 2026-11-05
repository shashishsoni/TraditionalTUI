//! Background events delivered to the app loop.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::acp::AcpManager;
use crate::git::GitFileStatus;
use crate::lsp::LspManager;
use crate::search::SearchMatch;

/// Message from a background subsystem.
pub enum AppEvent {
    FileIndex(Vec<PathBuf>),
    GitRefreshed {
        branch: Option<String>,
        statuses: HashMap<PathBuf, GitFileStatus>,
    },
    GitFailed(String),
    SearchResults {
        query: String,
        results: Vec<SearchMatch>,
    },
    SearchFailed(String),
    Error(String),
    AcpReady(Box<AcpManager>),
    LspReady(Box<LspManager>),
}
