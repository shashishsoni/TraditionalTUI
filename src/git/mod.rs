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

    #[error("path is outside the repository: {0}")]
    InvalidPath(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Line-level change category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LineChange {
    Added,
    Modified,
    Removed,
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

    /// Current branch short name including `HEAD` for detached states.
    pub fn branch_short(&self) -> String {
        self.branch
            .as_deref()
            .filter(|branch| !branch.is_empty() && *branch != "HEAD")
            .unwrap_or("HEAD")
            .to_string()
    }

    /// Whether this looks like a detached HEAD state.
    pub fn is_detached(&self) -> bool {
        self.branch.as_deref().is_none_or(|branch| branch == "HEAD")
    }

    /// Workspace-root-relative path for git object lookups.
    pub fn display_root_for(&self, _absolute: &Path) -> Option<PathBuf> {
        Some(self.root.clone())
    }

    /// Read a file from `HEAD`, returning `Ok(None)` when untracked or when
    /// git cannot produce the content. `git show` prints nothing for an empty
    /// HEAD file, so zero-byte output is `None`, not `Some("")`.
    pub fn head_content(&self, relative: &Path) -> Result<Option<String>, GitError> {
        let escaped = relative.to_string_lossy().replace('\\', "/");
        match run_git_bytes(&self.root, &["show", &format!("HEAD:{escaped}")]) {
            Ok(output) if output.is_empty() => Ok(None),
            Ok(output) => Ok(Some(String::from_utf8_lossy(&output).into_owned())),
            Err(GitError::Command(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Read the staged (index) version of a file.
    pub fn staged_content(&self, relative: &Path) -> Result<Option<String>, GitError> {
        let escaped = relative.to_string_lossy().replace('\\', "/");
        match run_git_bytes(&self.root, &["show", &format!(":{escaped}")]) {
            Ok(output) if output.is_empty() => Ok(None),
            Ok(output) => Ok(Some(String::from_utf8_lossy(&output).into_owned())),
            Err(GitError::Command(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Read a file's worktree bytes.
    pub fn worktree_content(path: &Path) -> std::io::Result<Option<String>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Untracked-file check driven by `git status`, so the gutter can render
    /// "every line added" even when `git show :path` reports nothing.
    fn is_untracked(&self, absolute: &Path) -> bool {
        self.statuses
            .get(absolute)
            .is_some_and(|status| *status == GitFileStatus::Untracked)
    }

    /// Per-file change marks: unstaged (worktree vs index) plus staged
    /// differences (index vs HEAD). Paths outside the repository root are
    /// treated as unknown content rather than an error. `root` anchors
    /// display-relative normalization for symlinked or remapped checkouts.
    pub fn working_tree_changes(&self, root: &Path, path: &Path) -> Vec<(usize, LineChange)> {
        let escaped = crate::platform::paths::relative_display(root, path);
        let relative: PathBuf = if escaped == path.to_string_lossy().replace('\\', "/") {
            // Outside the root (or already absolute): try stripping the root.
            match path.strip_prefix(root) {
                Ok(stripped) => stripped.to_path_buf(),
                Err(_) => return Vec::new(),
            }
        } else {
            PathBuf::from(escaped)
        };

        let head_file = self.head_content(&relative).ok().flatten();
        let staged_file = self.staged_content(&relative).ok().flatten();
        let worktree_file = Self::worktree_content(path).ok().flatten();

        let mut marks: Vec<(usize, LineChange)> = Vec::new();

        // Untracked files never exist in HEAD or the index: every worktree
        // line renders as an addition, so edits are visible on first keystroke.
        if self.is_untracked(path) {
            if let Some(work) = worktree_file.as_deref() {
                marks.extend((0..lines_of(work).len()).map(|line| (line, LineChange::Added)));
            }
            marks.sort_by_key(|(line, _)| *line);
            marks.dedup();
            return marks;
        }

        if let (Some(head), Some(staged)) = (head_file.as_deref(), staged_file.as_deref()) {
            if head != staged {
                marks.extend(diff_marks(&lines_of(head), &lines_of(staged), true));
            }
        }
        if let (Some(base), Some(work)) = (
            staged_file.as_deref().or(head_file.as_deref()),
            worktree_file.as_deref(),
        ) {
            if base != work {
                marks.extend(diff_marks(&lines_of(base), &lines_of(work), false));
            }
        }
        marks.sort_by_key(|(line, _)| *line);
        marks.dedup();
        marks
    }

    /// Per-file staged vs unstaged line counts, keyed by absolute path.
    /// `git show :path` can be noisy for exotic paths; failures produce an
    /// empty staging record rather than an error.
    pub fn staging_summary(&self) -> std::collections::HashMap<PathBuf, Staging> {
        let mut summary = std::collections::HashMap::new();
        let mut rel_paths: Vec<PathBuf> = self.statuses.keys().cloned().collect();
        rel_paths.sort();
        for absolute in rel_paths {
            let relative = self
                .relative_path(&absolute)
                .unwrap_or_else(|_| absolute.clone());
            let head = self.head_content(&relative).ok().flatten();
            let staged = self.staged_content(&relative).ok().flatten();
            let worktree = Self::worktree_content(&absolute).ok().flatten();

            let has_staged = staged.as_deref().is_some_and(|text| {
                !text.is_empty() || head.as_deref().is_some_and(|h| !h.is_empty())
            });
            let mut record = Staging::default();

            match (head.as_deref(), staged.as_deref()) {
                (Some(head_text), Some(staged_text)) if head_text != staged_text => {
                    record.staged_additions = lines_of(staged_text).len();
                }
                (None, Some(staged_text)) if has_staged => {
                    record.staged_additions = lines_of(staged_text).len();
                }
                (Some(_), Some(_)) => {}
                _ => {}
            }

            if let (Some(base), Some(work)) =
                (staged.as_deref().or(head.as_deref()), worktree.as_deref())
            {
                if base != work {
                    record.unstaged_additions = lines_of(work).len();
                }
            }

            // Untracked files: everything is unstaged.
            if self
                .statuses
                .get(&absolute)
                .is_some_and(|status| *status == GitFileStatus::Untracked)
            {
                record.unstaged_additions = worktree.map(|text| lines_of(&text).len()).unwrap_or(0);
            }

            summary.insert(absolute, record);
        }
        summary
    }

    /// Workspace-root-relative path for git object lookups.
    fn relative_path(&self, absolute: &Path) -> Result<PathBuf, GitError> {
        absolute
            .strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .map_err(|_| GitError::InvalidPath(absolute.display().to_string()))
    }

    /// Stage a file (`git add`).
    pub fn stage_path(&self, path: &Path) -> Result<(), GitError> {
        let relative = self.relative_path(path)?;
        let escaped = relative.to_string_lossy().replace('\\', "/");
        run_git(&self.root, &["add", "--", &escaped])?;
        Ok(())
    }

    /// Unstage a file but keep worktree edits (`git restore --staged`).
    pub fn unstage_path(&self, path: &Path) -> Result<(), GitError> {
        let relative = self.relative_path(path)?;
        let escaped = relative.to_string_lossy().replace('\\', "/");
        run_git(&self.root, &["restore", "--staged", "--", &escaped])?;
        Ok(())
    }

    /// Discard all worktree changes to a file (`git checkout -- <path>`).
    /// Used by tests to restore fixtures; the UI keeps edits lossless.
    #[allow(dead_code)]
    pub fn discard_path(&self, path: &Path) -> Result<(), GitError> {
        let relative = self.relative_path(path)?;
        let escaped = relative.to_string_lossy().replace('\\', "/");
        run_git(&self.root, &["checkout", "--", &escaped])?;
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

fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

#[cfg(test)]
#[allow(dead_code)]
fn relative_path(root: &Path, absolute: &Path) -> Result<PathBuf, GitError> {
    absolute
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| GitError::InvalidPath(absolute.display().to_string()))
}

/// Map imara-diff ranges to gutter line numbers. Staged ranges are reported
/// as `Added` (index additions); unstaged changes keep Added/Modified and
/// deletions map to the removed line anchor.
fn diff_marks(before: &[String], after: &[String], staged: bool) -> Vec<(usize, LineChange)> {
    use imara_diff::intern::InternedInput;
    use imara_diff::sources::lines;
    use imara_diff::{Algorithm, diff};

    let before_join = before.join("\n");
    let after_join = after.join("\n");
    let input = InternedInput::new(lines(&before_join), lines(&after_join));
    let mut marks = Vec::new();
    diff(
        Algorithm::Histogram,
        &input,
        |before_range: std::ops::Range<u32>, after_range: std::ops::Range<u32>| {
            if after_range.is_empty() {
                if !before_range.is_empty() && !staged {
                    marks.push((before_range.start as usize, LineChange::Removed));
                }
                return;
            }
            if before_range.is_empty() {
                let kind = LineChange::Added;
                for line in after_range.start as usize..after_range.end as usize {
                    marks.push((line, kind));
                }
                return;
            }
            let kind = if staged {
                LineChange::Added
            } else {
                LineChange::Modified
            };
            for line in after_range.start as usize..after_range.end as usize {
                marks.push((line, kind));
            }
        },
    );
    marks
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

/// Detailed per-file staging state for the Source Control panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Staging {
    pub staged_additions: usize,
    pub staged_deletions: usize,
    pub unstaged_additions: usize,
    pub unstaged_deletions: usize,
}

impl Staging {
    /// Whether this file has any staged index changes.
    pub fn has_staged(self) -> bool {
        self.staged_additions > 0 || self.staged_deletions > 0
    }

    /// Whether this file has any unstaged worktree changes.
    pub fn has_unstaged(self) -> bool {
        self.unstaged_additions > 0 || self.unstaged_deletions > 0
    }

    /// Short badge: `M` (unstaged), `M+` (staged+unstaged), `S`
    /// (staged only), `U` compound states.
    pub fn badge(self) -> &'static str {
        match (self.has_staged(), self.has_unstaged()) {
            (true, true) => "M+",
            (true, false) => "S",
            (false, true) => "M",
            (false, false) => " ",
        }
    }

    /// Bracketed label for the source control panel, e.g. `[S M+/3]`.
    pub fn detail(self) -> String {
        match (self.staged_additions, self.unstaged_additions) {
            (0, 0) => "clean".to_string(),
            (staged, 0) => format!("staged {staged}"),
            (0, unstaged) => format!("unstaged {unstaged}"),
            (staged, unstaged) => format!("staged {staged} + unstaged {unstaged}"),
        }
    }
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
    use tempfile::TempDir;

    /// Create a three-line repo with one staged and one unstaged edit.
    fn mixed_repo() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git")
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").expect("write");
        run(&["add", "a.txt"]);
        run(&["commit", "-qm", "init"]);
        std::fs::write(root.join("a.txt"), "one\nTWO-staged\nthree\n").expect("write");
        run(&["add", "a.txt"]);
        std::fs::write(
            root.join("a.txt"),
            "one\nTWO-staged\nthree\nfour-unstaged\n",
        )
        .expect("write");
        (dir, root.join("a.txt"))
    }

    #[test]
    fn marks_distinguish_staged_and_unstaged_changes() {
        let (_dir, file) = mixed_repo();
        let repository = GitRepository::discover(&file).expect("discover");
        let root = repository.root().to_path_buf();
        let mut marks = repository.working_tree_changes(&root, &file);
        marks.sort();
        // Line 1 (index 1) differs between HEAD and the index: staged.
        assert!(marks.contains(&(1, LineChange::Added)));
        // The new trailing line only exists in the worktree diff base.
        assert!(marks.contains(&(3, LineChange::Added)));
    }

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
    fn marks_untracked_files_as_added() {
        let (_dir, file) = mixed_repo();
        let extra = file.parent().expect("parent").join("extra.txt");
        std::fs::write(&extra, "one\ntwo\n").expect("write");
        let repository = GitRepository::discover(&extra).expect("discover");
        let mut repository = repository;
        repository.refresh().expect("refresh");
        assert_eq!(
            repository.status_for(&extra),
            Some(GitFileStatus::Untracked)
        );
        let root = repository.root().to_path_buf();
        assert_eq!(
            repository.working_tree_changes(&root, &extra),
            vec![(0, LineChange::Added), (1, LineChange::Added)]
        );
    }

    #[test]
    fn untracked_files_show_all_unstaged_changes() {
        let (_dir, file) = mixed_repo();
        let mut repository = GitRepository::discover(&file).expect("discover");
        let extra = repository.root().join("extra.txt");
        std::fs::write(&extra, "one\ntwo\n").expect("write");
        repository.refresh().expect("refresh");
        let summary = repository.staging_summary();
        let record = summary.get(&extra).expect("record");
        assert_eq!(record.unstaged_additions, 2);
        assert_eq!(record.staged_additions, 0);
    }

    #[test]
    fn staging_summary_counts_staged_edits() {
        let (_dir, file) = mixed_repo();
        let mut repository = GitRepository::discover(&file).expect("discover");
        repository.refresh().expect("refresh");
        let summary = repository.staging_summary();
        let record = summary.get(&file).expect("record");
        assert!(record.has_staged());
        assert!(record.has_unstaged());
        assert_eq!(record.badge(), "M+");
    }

    #[test]
    fn diff_marks_empty_inputs() {
        assert!(diff_marks(&[], &[], false).is_empty());
        assert!(diff_marks(&[], &["only".to_string()], false) == vec![(0, LineChange::Added)]);
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
