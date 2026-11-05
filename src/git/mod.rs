//! Git repository discovery, branch/status inspection and gutter diffs.
//!
//! Status inspection shells out to the user's `git` binary with piped stdio so
//! repository-local output can never contaminate the TUI terminal. All calls
//! are opt-in from the app's background worker.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use thiserror::Error;

use crate::platform::process::configure_background_command;

/// Per-file Git status shown in the explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitFileStatus {
    /// One-character badge used by the explorer.
    pub fn badge(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Untracked => '?',
            Self::Conflict => '!',
        }
    }
}

/// Git failure.
#[derive(Debug, Error)]
pub enum GitError {
    #[error("git command failed: {0}")]
    Command(String),

    #[error("git is not installed or not on PATH")]
    NotInstalled,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// A discovered repository.
#[derive(Debug, Clone)]
pub struct GitRepository {
    root: PathBuf,
    branch: Option<String>,
    statuses: HashMap<PathBuf, GitFileStatus>,
}

impl GitRepository {
    /// Walk up from `start` until a `.git` entry is found.
    pub fn discover(start: impl AsRef<Path>) -> Option<Self> {
        let start = start.as_ref();
        let mut current = if start.is_dir() {
            start.to_path_buf()
        } else {
            start.parent()?.to_path_buf()
        };
        loop {
            if current.join(".git").exists() {
                return Some(Self {
                    root: current,
                    branch: None,
                    statuses: HashMap::new(),
                });
            }
            if !current.pop() {
                return None;
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn statuses(&self) -> &HashMap<PathBuf, GitFileStatus> {
        &self.statuses
    }

    pub fn status_for(&self, path: &Path) -> Option<GitFileStatus> {
        self.statuses.get(path).copied()
    }

    /// Apply a background refresh result.
    pub fn apply(
        &mut self,
        branch: Option<String>,
        statuses: std::collections::HashMap<PathBuf, GitFileStatus>,
    ) {
        self.branch = branch;
        self.statuses = statuses;
    }

    /// List local branch names.
    pub fn list_branches(&self) -> Result<Vec<String>, GitError> {
        let output = run_git(
            &self.root,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )?;
        let mut branches: Vec<String> = output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        branches.sort();
        Ok(branches)
    }

    /// Check out an existing local branch.
    pub fn switch_branch(&self, branch: &str) -> Result<(), GitError> {
        run_git(&self.root, &["checkout", "--no-guess", branch])?;
        Ok(())
    }

    /// Refresh branch and file status information.
    pub fn refresh(&mut self) -> Result<(), GitError> {
        let branch = run_git(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let branch = branch.trim();
        self.branch = (!branch.is_empty() && branch != "HEAD").then(|| branch.to_string());

        let output = run_git_bytes(
            &self.root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?;
        self.statuses = parse_porcelain(&self.root, &output);
        Ok(())
    }

    /// Line marks for the gutter comparing the on-disk text to the buffer.
    pub fn gutter_diff(
        &self,
        disk_text: &str,
        buffer_text: &str,
    ) -> Vec<(usize, crate::editor::GutterMark)> {
        use crate::editor::GutterMark;
        use imara_diff::intern::InternedInput;
        use imara_diff::sources::lines;
        use imara_diff::{Algorithm, diff};

        let input = InternedInput::new(lines(disk_text), lines(buffer_text));
        let mut marks: Vec<(usize, GutterMark)> = Vec::new();
        diff(
            Algorithm::Histogram,
            &input,
            |before: std::ops::Range<u32>, after: std::ops::Range<u32>| {
                if after.is_empty() {
                    if !before.is_empty() {
                        marks.push((after.start as usize, GutterMark::Deleted));
                    }
                    return;
                }
                let kind = if before.is_empty() {
                    GutterMark::Added
                } else {
                    GutterMark::Modified
                };
                let before_len = (before.end - before.start) as usize;
                let after_len = (after.end - after.start) as usize;
                let modified = before_len.min(after_len);
                for offset in 0..modified {
                    marks.push((after.start as usize + offset, kind));
                }
                for offset in modified..after_len {
                    marks.push((after.start as usize + offset, GutterMark::Added));
                }
            },
        );
        marks.sort_by_key(|(line, _)| *line);
        marks.dedup_by_key(|(line, _)| *line);
        marks
    }
}

fn run_git(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let bytes = run_git_bytes(root, args)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run_git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_background_command(&mut command);
    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Io(error)
        }
    })?;
    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(output.stdout)
}

fn parse_porcelain(root: &Path, bytes: &[u8]) -> HashMap<PathBuf, GitFileStatus> {
    let mut statuses = HashMap::new();
    let mut parts = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let x = entry[0] as char;
        let y = entry[1] as char;
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        let status = classify(x, y);
        statuses.insert(root.join(path), status);
        if matches!(x, 'R' | 'C') {
            // Rename/copy entries carry the original path as the next record.
            let _ = parts.next();
        }
    }
    statuses
}

fn classify(x: char, y: char) -> GitFileStatus {
    if x == '?' || y == '?' {
        return GitFileStatus::Untracked;
    }
    if x == 'U' || y == 'U' {
        return GitFileStatus::Conflict;
    }
    match (x, y) {
        ('A', _) | (_, 'A') => GitFileStatus::Added,
        ('D', _) | (_, 'D') => GitFileStatus::Deleted,
        ('R', _) => GitFileStatus::Renamed,
        ('C', _) => GitFileStatus::Added,
        _ => GitFileStatus::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_entries() {
        let root = Path::new("/repo");
        let statuses = parse_porcelain(root, b" M src/main.rs\0?? notes.md\0A  new.rs\0");
        assert_eq!(statuses[&root.join("src/main.rs")], GitFileStatus::Modified);
        assert_eq!(statuses[&root.join("notes.md")], GitFileStatus::Untracked);
        assert_eq!(statuses[&root.join("new.rs")], GitFileStatus::Added);
    }

    #[test]
    fn gutter_marks_added_and_modified_lines() {
        let repository = GitRepository {
            root: PathBuf::from("/repo"),
            branch: None,
            statuses: HashMap::new(),
        };
        let marks = repository.gutter_diff("a\nb\n", "a\nB\nc\n");
        assert!(
            marks
                .iter()
                .any(|(_, kind)| *kind == crate::editor::GutterMark::Modified)
        );
        assert!(
            marks
                .iter()
                .any(|(_, kind)| *kind == crate::editor::GutterMark::Added)
        );
    }

    #[test]
    fn discover_walks_up_to_repo_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir");
        let nested = dir.path().join("src").join("nested");
        std::fs::create_dir_all(&nested).expect("mkdir");
        let repository = GitRepository::discover(&nested).expect("discover");
        assert_eq!(
            crate::platform::paths::canonicalize_clean(repository.root()).ok(),
            crate::platform::paths::canonicalize_clean(dir.path()).ok()
        );
    }
}
