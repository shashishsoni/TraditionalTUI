//! Diagnostics store keyed by document URI.

use std::collections::HashMap;

/// Severity mirrored from LSP numeric codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

impl Severity {
    pub fn from_lsp(value: u64) -> Self {
        match value {
            1 => Self::Error,
            2 => Self::Warning,
            3 => Self::Information,
            _ => Self::Hint,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "E",
            Self::Warning => "W",
            Self::Information => "I",
            Self::Hint => "H",
        }
    }
}

/// One normalized diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticEntry {
    pub uri: String,
    pub line: u32,
    pub character: u32,
    pub severity: Severity,
    pub message: String,
    pub source: Option<String>,
}

/// Per-URI diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticStore {
    entries: HashMap<String, Vec<DiagnosticEntry>>,
}

impl DiagnosticStore {
    pub fn set(&mut self, uri: impl Into<String>, entries: Vec<DiagnosticEntry>) {
        let uri = uri.into();
        if entries.is_empty() {
            self.entries.remove(&uri);
        } else {
            self.entries.insert(uri, entries);
        }
    }

    pub fn clear(&mut self, uri: &str) {
        self.entries.remove(uri);
    }

    pub fn get(&self, uri: &str) -> &[DiagnosticEntry] {
        self.entries.get(uri).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn all(&self) -> impl Iterator<Item = &DiagnosticEntry> {
        self.entries.values().flatten()
    }

    pub fn counts(&self) -> (usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        for entry in self.all() {
            match entry.severity {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
                _ => {}
            }
        }
        (errors, warnings)
    }

    pub fn total(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(severity: Severity) -> DiagnosticEntry {
        DiagnosticEntry {
            uri: "file:///a.rs".into(),
            line: 0,
            character: 0,
            severity,
            message: "m".into(),
            source: None,
        }
    }

    #[test]
    fn counts_errors_and_warnings() {
        let mut store = DiagnosticStore::default();
        store.set(
            "file:///a.rs",
            vec![
                entry(Severity::Error),
                entry(Severity::Warning),
                entry(Severity::Hint),
            ],
        );
        assert_eq!(store.counts(), (1, 1));
        assert_eq!(store.total(), 3);
    }

    #[test]
    fn empty_update_clears_uri() {
        let mut store = DiagnosticStore::default();
        store.set("file:///a.rs", vec![entry(Severity::Error)]);
        store.set("file:///a.rs", Vec::new());
        assert!(store.is_empty());
    }
}
