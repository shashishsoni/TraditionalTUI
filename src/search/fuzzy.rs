//! `nucleo`-backed fuzzy file picker.

use nucleo_matcher::{Config, Matcher, Utf32Str};

/// One scored fuzzy match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub index: usize,
    pub score: u16,
}

/// Fuzzy matcher over a fixed item list.
#[derive(Debug, Clone)]
pub struct FuzzyPicker {
    matcher: Matcher,
    items: Vec<String>,
}

impl Default for FuzzyPicker {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl FuzzyPicker {
    pub fn new(items: Vec<String>) -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            items,
        }
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Return the best `limit` matches for `query`, best score first.
    pub fn filter(&mut self, query: &str, limit: usize) -> Vec<FuzzyMatch> {
        if query.trim().is_empty() {
            return self
                .items
                .iter()
                .take(limit)
                .enumerate()
                .map(|(index, _)| FuzzyMatch { index, score: 0 })
                .collect();
        }

        let mut needle_buffer = Vec::new();
        let needle = Utf32Str::new(query, &mut needle_buffer);

        let mut results = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            let mut haystack_buffer = Vec::new();
            let haystack = Utf32Str::new(item, &mut haystack_buffer);
            if let Some(score) = self.matcher.fuzzy_match(haystack, needle) {
                results.push(FuzzyMatch { index, score });
            }
        }
        results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
        results.truncate(limit);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_filter_ranks_contiguous_matches_first() {
        let mut picker = FuzzyPicker::new(vec![
            "src/editor/document.rs".into(),
            "src/editor/cursor.rs".into(),
            "docs/EDITOR_ENGINE.md".into(),
        ]);
        let matches = picker.filter("editor", 10);
        assert!(!matches.is_empty());
        assert!(picker.items()[matches[0].index].contains("editor"));
    }

    #[test]
    fn empty_query_returns_everything_up_to_limit() {
        let mut picker = FuzzyPicker::new(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(picker.filter("", 2).len(), 2);
    }
}
