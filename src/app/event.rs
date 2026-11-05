//! Background events delivered to the app loop.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::acp::AcpManager;
use crate::git::GitFileStatus;
use crate::lsp::LspManager;
use crate::search::SearchMatch;

/// Message from a background subsystem.
pub enum AppEvent {
    FileIndex {
        root: PathBuf,
        files: Vec<PathBuf>,
    },
    GitRefreshed {
        root: PathBuf,
        branch: Option<String>,
        statuses: HashMap<PathBuf, GitFileStatus>,
    },
    GitFailed {
        root: PathBuf,
        error: String,
    },
    GitGutter {
        root: PathBuf,
        version: u64,
        marks: Vec<(usize, crate::git::LineChange)>,
    },
    SearchResults {
        query: String,
        results: Vec<SearchMatch>,
    },
    SearchFailed(String),
    Notice(String),
    Error(String),
    Branches {
        branches: Vec<String>,
        current: Option<String>,
    },
    BranchesFailed(String),
    AcpReady {
        manager: Box<AcpManager>,
        generation: u64,
    },
    LspReady {
        manager: Box<LspManager>,
        generation: u64,
    },
}
