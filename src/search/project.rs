//! Streaming project-wide regex search built on the `ignore` and
//! `grep-searcher` engines.

use std::path::{Path, PathBuf};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use thiserror::Error;

/// One project search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub line: u64,
    pub text: String,
}

/// Project search failure.
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("invalid search pattern: {0}")]
    Pattern(#[from] grep_regex::Error),

    #[error("search I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Walk the project honoring ignore files, returning up to `max` file paths.
pub fn index_files(root: &Path, max: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .build();
    for entry in walker.flatten() {
        if files.len() >= max {
            break;
        }
        let path = entry.path();
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && !path.starts_with(root.join(".git"))
        {
            files.push(path.to_path_buf());
        }
    }
    files
}

/// Search `root` for `query` (regex), returning up to `max` matches.
pub fn search_project(
    root: &Path,
    query: &str,
    case_sensitive: bool,
    max: usize,
) -> Result<Vec<SearchMatch>, SearchError> {
    if query.is_empty() || max == 0 {
        return Ok(Vec::new());
    }

    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .build(query)?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .build();

    let mut matches = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .build();

    for entry in walker.flatten() {
        if matches.len() >= max {
            break;
        }
        let path = entry.path();
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let mut sink = MatchSink {
            path: path.to_path_buf(),
            matches: &mut matches,
            max,
        };
        searcher.search_path(&matcher, path, &mut sink)?;
    }

    Ok(matches)
}

struct MatchSink<'a> {
    path: PathBuf,
    matches: &'a mut Vec<SearchMatch>,
    max: usize,
}

impl Sink for MatchSink<'_> {
    type Error = std::io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let text = String::from_utf8_lossy(mat.bytes())
            .trim_end_matches(['\n', '\r'])
            .to_string();
        self.matches.push(SearchMatch {
            path: self.path.clone(),
            line: mat.line_number().unwrap_or(0),
            text,
        });
        Ok(self.matches.len() < self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_files_and_respects_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "needle").expect("write");
        std::fs::write(dir.path().join("b.txt"), "hay").expect("write");
        let files = index_files(dir.path(), 10);
        assert_eq!(files.len(), 2);
        assert_eq!(index_files(dir.path(), 1).len(), 1);
    }

    #[test]
    fn search_finds_matches_with_line_numbers() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "first\nneedle\nlast\n").expect("write");
        let matches = search_project(dir.path(), "needle", true, 10).expect("search");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].line, 2);
        assert_eq!(matches[0].text, "needle");
    }

    #[test]
    fn search_is_case_insensitive_when_requested() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "Needle\n").expect("write");
        assert_eq!(
            search_project(dir.path(), "needle", false, 10)
                .expect("search")
                .len(),
            1
        );
        assert!(
            search_project(dir.path(), "needle", true, 10)
                .expect("search")
                .is_empty()
        );
    }

    #[test]
    fn invalid_regex_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = search_project(dir.path(), "(", true, 10).expect_err("must fail");
        assert!(matches!(error, SearchError::Pattern(_)));
    }
}
