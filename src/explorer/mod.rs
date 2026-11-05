//! Lazy hierarchical file explorer model.
//!
//! The model is pure state: it reads directories on demand, tracks expansion,
//! selection and Git decorations, and exposes a flat list for the renderer.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::git::GitFileStatus;
use crate::platform::paths::canonicalize_clean;

/// Explorer failure.
#[derive(Debug, Error)]
pub enum ExplorerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid file name: {0}")]
    InvalidName(String),

    #[error("path is outside of the workspace: {0}")]
    OutsideWorkspace(PathBuf),
}

/// One node in the file tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub children: Option<Vec<TreeNode>>,
    pub git: Option<GitFileStatus>,
}

impl TreeNode {
    fn new(path: PathBuf, is_dir: bool, git: Option<GitFileStatus>) -> Self {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        Self {
            path,
            name,
            is_dir,
            expanded: false,
            children: None,
            git,
        }
    }
}

/// Flattened row ready for rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub git: Option<GitFileStatus>,
}

/// File tree model.
#[derive(Debug)]
pub struct FileExplorer {
    root: PathBuf,
    entries: Vec<TreeNode>,
    selected: usize,
    show_hidden: bool,
    statuses: HashMap<PathBuf, GitFileStatus>,
}

impl FileExplorer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ExplorerError> {
        let root =
            canonicalize_clean(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
        let mut explorer = Self {
            root,
            entries: Vec::new(),
            selected: 0,
            show_hidden: false,
            statuses: HashMap::new(),
        };
        explorer.refresh()?;
        Ok(explorer)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) {
        if self.show_hidden != show_hidden {
            self.show_hidden = show_hidden;
            let _ = self.refresh();
        }
    }

    /// Reload the root, preserving expanded directories where possible.
    pub fn refresh(&mut self) -> Result<(), ExplorerError> {
        let expanded: HashSet<PathBuf> = self.expanded_paths().into_iter().collect();
        self.entries = self.read_dir(&self.root)?;
        for path in expanded {
            let _ = self.expand(&path);
        }
        self.clamp_selection();
        Ok(())
    }

    pub fn entries(&self) -> &[TreeNode] {
        &self.entries
    }

    /// Rows visible after applying expansion state.
    pub fn visible_entries(&self) -> Vec<VisibleEntry> {
        let mut rows = Vec::new();
        flatten(&self.entries, 0, &mut rows);
        rows
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn set_selected(&mut self, index: usize) {
        self.selected = index.min(self.visible_entries().len().saturating_sub(1));
    }

    pub fn selected_entry(&self) -> Option<VisibleEntry> {
        self.visible_entries().get(self.selected).cloned()
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_entry().map(|entry| entry.path)
    }

    pub fn select_next(&mut self) {
        let count = self.visible_entries().len();
        if count > 0 {
            self.selected = (self.selected + 1).min(count - 1);
        }
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Select the row for `path` when it is visible.
    pub fn select_path(&mut self, path: &Path) {
        if let Some(index) = self
            .visible_entries()
            .iter()
            .position(|entry| entry.path == path)
        {
            self.selected = index;
        }
    }

    pub fn expand(&mut self, path: &Path) -> Result<(), ExplorerError> {
        let is_dir = find_node(&self.entries, path).is_some_and(|node| node.is_dir);
        if !is_dir {
            return Ok(());
        }
        let needs_load = find_node(&self.entries, path).is_some_and(|node| node.children.is_none());
        let children = if needs_load {
            Some(self.read_dir(path)?)
        } else {
            None
        };
        let Some(node) = find_node_mut(&mut self.entries, path) else {
            return Ok(());
        };
        if let Some(children) = children {
            node.children = Some(children);
        }
        node.expanded = true;
        self.clamp_selection();
        Ok(())
    }

    pub fn collapse(&mut self, path: &Path) {
        if let Some(node) = find_node_mut(&mut self.entries, path) {
            node.expanded = false;
        }
        self.clamp_selection();
    }

    pub fn toggle(&mut self, path: &Path) -> Result<(), ExplorerError> {
        let is_expanded = find_node(&self.entries, path).is_some_and(|node| node.expanded);
        if is_expanded {
            self.collapse(path);
            Ok(())
        } else {
            self.expand(path)
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        find_node(&self.entries, path).is_some_and(|node| node.expanded)
    }

    /// Toggle expansion of the currently selected directory.
    pub fn toggle_selected(&mut self) -> Result<(), ExplorerError> {
        if let Some(entry) = self.selected_entry() {
            if entry.is_dir {
                self.toggle(&entry.path)?;
            }
        }
        Ok(())
    }

    /// Apply Git status decorations.
    pub fn set_git_statuses(&mut self, statuses: HashMap<PathBuf, GitFileStatus>) {
        self.statuses = statuses;
        apply_statuses(&mut self.entries, &self.statuses);
    }

    pub fn statuses(&self) -> &HashMap<PathBuf, GitFileStatus> {
        &self.statuses
    }

    pub fn create_file(
        &mut self,
        parent: &Path,
        name: &str,
        contents: &str,
    ) -> Result<PathBuf, ExplorerError> {
        self.ensure_inside(parent)?;
        let path = parent.join(validate_name(name)?);
        std::fs::write(&path, contents)?;
        self.refresh()?;
        self.select_path(&path);
        Ok(path)
    }

    pub fn create_directory(
        &mut self,
        parent: &Path,
        name: &str,
    ) -> Result<PathBuf, ExplorerError> {
        self.ensure_inside(parent)?;
        let path = parent.join(validate_name(name)?);
        std::fs::create_dir(&path)?;
        self.refresh()?;
        self.select_path(&path);
        Ok(path)
    }

    pub fn rename(&mut self, path: &Path, new_name: &str) -> Result<PathBuf, ExplorerError> {
        self.ensure_inside(path)?;
        let parent = path.parent().unwrap_or(&self.root);
        let target = parent.join(validate_name(new_name)?);
        std::fs::rename(path, &target)?;
        self.refresh()?;
        self.select_path(&target);
        Ok(target)
    }

    pub fn delete(&mut self, path: &Path) -> Result<(), ExplorerError> {
        self.ensure_inside(path)?;
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        self.refresh()?;
        Ok(())
    }

    fn ensure_inside(&self, path: &Path) -> Result<(), ExplorerError> {
        if path.starts_with(&self.root) {
            Ok(())
        } else {
            Err(ExplorerError::OutsideWorkspace(path.to_path_buf()))
        }
    }

    fn read_dir(&self, dir: &Path) -> Result<Vec<TreeNode>, ExplorerError> {
        let mut nodes = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || (!self.show_hidden && name.starts_with('.')) {
                continue;
            }
            let file_type = entry.file_type()?;
            let git = self.statuses.get(&path).copied();
            nodes.push(TreeNode::new(path, file_type.is_dir(), git));
        }
        nodes.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(nodes)
    }

    fn expanded_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        collect_expanded(&self.entries, &mut paths);
        paths
    }

    fn clamp_selection(&mut self) {
        let count = self.visible_entries().len();
        if count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(count - 1);
        }
    }
}

fn flatten(nodes: &[TreeNode], depth: usize, rows: &mut Vec<VisibleEntry>) {
    for node in nodes {
        rows.push(VisibleEntry {
            path: node.path.clone(),
            name: node.name.clone(),
            depth,
            is_dir: node.is_dir,
            expanded: node.expanded,
            git: node.git,
        });
        if node.is_dir && node.expanded {
            if let Some(children) = &node.children {
                flatten(children, depth + 1, rows);
            }
        }
    }
}

fn collect_expanded(nodes: &[TreeNode], paths: &mut Vec<PathBuf>) {
    for node in nodes {
        if node.is_dir && node.expanded {
            paths.push(node.path.clone());
        }
        if let Some(children) = &node.children {
            collect_expanded(children, paths);
        }
    }
}

fn find_node<'a>(nodes: &'a [TreeNode], path: &Path) -> Option<&'a TreeNode> {
    for node in nodes {
        if node.path == path {
            return Some(node);
        }
        if let Some(children) = node.children.as_deref() {
            if let Some(found) = find_node(children, path) {
                return Some(found);
            }
        }
    }
    None
}

fn find_node_mut<'a>(nodes: &'a mut [TreeNode], path: &Path) -> Option<&'a mut TreeNode> {
    for node in nodes.iter_mut() {
        if node.path == path {
            return Some(node);
        }
        if let Some(children) = node.children.as_mut() {
            if let Some(found) = find_node_mut(children, path) {
                return Some(found);
            }
        }
    }
    None
}

fn apply_statuses(nodes: &mut [TreeNode], statuses: &HashMap<PathBuf, GitFileStatus>) {
    for node in nodes {
        node.git = statuses.get(&node.path).copied();
        if let Some(children) = node.children.as_mut() {
            apply_statuses(children, statuses);
        }
    }
}

fn validate_name(name: &str) -> Result<&str, ExplorerError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(ExplorerError::InvalidName(name.to_string()));
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        std::fs::create_dir(dir.path().join("src")).expect("mkdir");
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").expect("write");
        dir
    }

    #[test]
    fn root_is_sorted_directories_first() {
        let dir = fixture();
        let explorer = FileExplorer::new(dir.path()).expect("explorer");
        let rows = explorer.visible_entries();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_dir);
        assert_eq!(rows[0].name, "src");
        assert_eq!(rows[1].name, "a.txt");
    }

    #[test]
    fn expanding_and_collapsing_changes_visible_rows() {
        let dir = fixture();
        let mut explorer = FileExplorer::new(dir.path()).expect("explorer");
        let root_child = explorer.visible_entries()[0].path.clone();
        assert_eq!(explorer.visible_entries().len(), 2);
        explorer.expand(&root_child).expect("expand");
        assert_eq!(explorer.visible_entries().len(), 3);
        explorer.collapse(&root_child);
        assert_eq!(explorer.visible_entries().len(), 2);
    }

    #[test]
    fn create_rename_and_delete_round_trip() {
        let dir = fixture();
        let mut explorer = FileExplorer::new(dir.path()).expect("explorer");
        let created = explorer
            .create_file(dir.path(), "new.txt", "hello")
            .expect("create");
        assert!(created.exists());
        let renamed = explorer.rename(&created, "renamed.txt").expect("rename");
        assert!(renamed.exists());
        explorer.delete(&renamed).expect("delete");
        assert!(!renamed.exists());
    }

    #[test]
    fn invalid_names_are_rejected() {
        let dir = fixture();
        let mut explorer = FileExplorer::new(dir.path()).expect("explorer");
        assert!(explorer.create_file(dir.path(), "../escape", "").is_err());
        assert!(explorer.create_file(dir.path(), "a/b", "").is_err());
    }

    #[test]
    fn git_statuses_decorate_nodes() {
        let dir = fixture();
        let mut explorer = FileExplorer::new(dir.path()).expect("explorer");
        let mut statuses = HashMap::new();
        statuses.insert(dir.path().join("a.txt"), GitFileStatus::Modified);
        explorer.set_git_statuses(statuses);
        assert_eq!(
            explorer
                .visible_entries()
                .iter()
                .find(|entry| entry.name == "a.txt")
                .and_then(|entry| entry.git),
            Some(GitFileStatus::Modified)
        );
    }
}
