//! Workspace state: project root, open document registry and session
//! persistence.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::editor::{Document, EditorError};
use crate::platform::paths::{PlatformPaths, canonicalize_clean, relative_display};

mod trust;

pub use trust::TrustStore;

/// Workspace failure.
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("editor error: {0}")]
    Editor(#[from] EditorError),

    #[error("workspace root does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),

    #[error("session file is invalid: {0}")]
    Session(#[from] serde_json::Error),
}

/// Persisted open-file session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default)]
    pub open_files: Vec<String>,
    #[serde(default)]
    pub active: usize,
}

/// Owns every open document for one project root.
#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    documents: Vec<Document>,
    active: usize,
    trust: TrustStore,
}

impl Workspace {
    /// Create a workspace for `root`, resolving symlinks.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(WorkspaceError::InvalidRoot(root.to_path_buf()));
        }
        let root = canonicalize_clean(root).unwrap_or_else(|_| root.to_path_buf());
        let trust = TrustStore::load(
            PlatformPaths::discover()
                .state_dir()
                .join("trusted_workspaces.json"),
        );
        Ok(Self {
            root,
            documents: Vec::new(),
            active: 0,
            trust,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn documents_mut(&mut self) -> &mut [Document] {
        &mut self.documents
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn active_index(&self) -> Option<usize> {
        if self.documents.is_empty() {
            None
        } else {
            Some(self.active.min(self.documents.len() - 1))
        }
    }

    pub fn active(&self) -> Option<&Document> {
        self.active_index().map(|index| &self.documents[index])
    }

    pub fn active_mut(&mut self) -> Option<&mut Document> {
        let index = self.active_index()?;
        self.documents.get_mut(index)
    }

    pub fn document(&self, index: usize) -> Option<&Document> {
        self.documents.get(index)
    }

    pub fn document_mut(&mut self, index: usize) -> Option<&mut Document> {
        self.documents.get_mut(index)
    }

    pub fn switch_to(&mut self, index: usize) {
        if index < self.documents.len() {
            self.active = index;
        }
    }

    /// Open an existing file, reusing an already-open buffer.
    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<usize, WorkspaceError> {
        let path = path.as_ref();
        let canonical = canonicalize_clean(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(index) = self.find_by_path(&canonical) {
            self.active = index;
            return Ok(index);
        }
        let document = Document::open(&canonical)?;
        Ok(self.push_document(document))
    }

    /// Add an already-created document.
    pub fn push_document(&mut self, document: Document) -> usize {
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        self.documents.len() - 1
    }

    /// Add a new scratch buffer.
    pub fn scratch(&mut self) -> usize {
        self.push_document(Document::scratch())
    }

    pub fn close(&mut self, index: usize) -> Option<Document> {
        if index >= self.documents.len() {
            return None;
        }
        let document = self.documents.remove(index);
        if self.active >= self.documents.len() && !self.documents.is_empty() {
            self.active = self.documents.len() - 1;
        }
        Some(document)
    }

    pub fn save_active(&mut self) -> Result<(), WorkspaceError> {
        if let Some(document) = self.active_mut() {
            document.save()?;
        }
        Ok(())
    }

    pub fn save_all(&mut self) -> Result<(), WorkspaceError> {
        for document in &mut self.documents {
            if document.dirty && document.path.is_some() {
                document.save()?;
            }
        }
        Ok(())
    }

    pub fn dirty_count(&self) -> usize {
        self.documents
            .iter()
            .filter(|document| document.dirty)
            .count()
    }

    pub fn find_by_path(&self, path: &Path) -> Option<usize> {
        let canonical = canonicalize_clean(path).unwrap_or_else(|_| path.to_path_buf());
        self.documents
            .iter()
            .position(|document| document.path.as_deref() == Some(canonical.as_path()))
    }

    pub fn display_path(&self, path: &Path) -> String {
        relative_display(&self.root, path)
    }

    /// Whether executable workspace configuration is trusted.
    pub fn is_trusted(&self) -> bool {
        self.trust.is_trusted(&self.root)
    }

    /// Mark the workspace configuration as trusted.
    pub fn grant_trust(&mut self) -> Result<(), WorkspaceError> {
        self.trust.trust(&self.root)?;
        Ok(())
    }

    pub fn trust_store(&self) -> &TrustStore {
        &self.trust
    }

    fn session_path(&self) -> PathBuf {
        self.root.join(".ttui").join("state.json")
    }

    /// Restore open files recorded by the previous session.
    pub fn load_session(&mut self) -> Result<usize, WorkspaceError> {
        let path = self.session_path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        let session: SessionState = serde_json::from_slice(&bytes)?;
        let mut opened = 0;
        for file in session.open_files {
            let path = self.root.join(file);
            if path.is_file() && self.open(&path).is_ok() {
                opened += 1;
            }
        }
        if session.active < self.documents.len() {
            self.active = session.active;
        }
        Ok(opened)
    }

    /// Persist open file paths and the active tab.
    pub fn save_session(&self) -> Result<(), WorkspaceError> {
        let state = SessionState {
            open_files: self
                .documents
                .iter()
                .filter_map(|document| document.path.as_deref().map(|path| self.display_path(path)))
                .filter(|path| !path.is_empty())
                .collect(),
            active: self.active,
        };
        let path = self.session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = serde_json::to_vec_pretty(&state)?;
        bytes.push(b'\n');
        crate::platform::fs::atomic_write(&path, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reuses_existing_buffer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "hello").expect("write");
        let mut workspace = Workspace::new(dir.path()).expect("workspace");
        let first = workspace.open(&file).expect("open");
        let second = workspace.open(&file).expect("open again");
        assert_eq!(first, second);
        assert_eq!(workspace.len(), 1);
    }

    #[test]
    fn close_adjusts_active_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = Workspace::new(dir.path()).expect("workspace");
        workspace.scratch();
        workspace.scratch();
        assert_eq!(workspace.active_index(), Some(1));
        workspace.close(1);
        assert_eq!(workspace.active_index(), Some(0));
    }

    #[test]
    fn session_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "hello").expect("write");

        let mut workspace = Workspace::new(dir.path()).expect("workspace");
        workspace.open(&file).expect("open");
        workspace.save_session().expect("save session");
        drop(workspace);

        let mut restored = Workspace::new(dir.path()).expect("workspace");
        assert_eq!(restored.load_session().expect("load"), 1);
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn invalid_root_is_rejected() {
        let error = Workspace::new("/definitely/not/a/real/directory/ttui").expect_err("must fail");
        assert!(matches!(error, WorkspaceError::InvalidRoot(_)));
    }
}
