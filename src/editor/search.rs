//! Buffer-local incremental search.

use std::ops::Range;

use ropey::Rope;

/// Find every occurrence of `needle`, returning character ranges.
pub fn find_all(rope: &Rope, needle: &str, case_sensitive: bool) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<char> = rope.chars().collect();
    let pattern: Vec<char> = needle.chars().collect();
    let mut matches = Vec::new();
    if pattern.len() > haystack.len() {
        return matches;
    }

    let last_start = haystack.len() - pattern.len();
    let mut start = 0;
    while start <= last_start {
        let matching = pattern
            .iter()
            .zip(&haystack[start..start + pattern.len()])
            .all(|(expected, actual)| chars_equal(*expected, *actual, case_sensitive));
        if matching {
            matches.push(start..start + pattern.len());
            start += pattern.len();
        } else {
            start += 1;
        }
    }
    matches
}

fn chars_equal(expected: char, actual: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        expected == actual
    } else {
        expected.eq_ignore_ascii_case(&actual)
    }
}

/// Interactive buffer search state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BufferSearch {
    needle: String,
    case_sensitive: bool,
    matches: Vec<Range<usize>>,
    current: usize,
}

impl BufferSearch {
    pub fn set_query(&mut self, rope: &Rope, needle: &str, case_sensitive: bool) {
        self.needle = needle.to_string();
        self.case_sensitive = case_sensitive;
        self.matches = find_all(rope, needle, case_sensitive);
        self.current = 0;
    }

    pub fn needle(&self) -> &str {
        &self.needle
    }

    pub fn matches(&self) -> &[Range<usize>] {
        &self.matches
    }

    pub fn count(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn current_match(&self) -> Option<Range<usize>> {
        self.matches.get(self.current).cloned()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn next_match(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.current_match()
    }

    pub fn previous_match(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        self.current_match()
    }

    /// Find the match at or after `position`.
    pub fn seek(&mut self, position: usize) -> Option<Range<usize>> {
        let index = self
            .matches
            .iter()
            .position(|range| range.start >= position)?;
        self.current = index;
        self.current_match()
    }

    pub fn clear(&mut self) {
        self.needle.clear();
        self.matches.clear();
        self.current = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_non_overlapping_matches() {
        let rope = Rope::from_str("aaaa");
        let matches = find_all(&rope, "aa", true);
        assert_eq!(matches, vec![0..2, 2..4]);
    }

    #[test]
    fn ascii_case_insensitive_matching() {
        let rope = Rope::from_str("Hello hello HELLO");
        let matches = find_all(&rope, "hello", false);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn unicode_offsets_are_char_indexed() {
        let rope = Rope::from_str("🦀 rust 🦀");
        let matches = find_all(&rope, "rust", true);
        assert_eq!(matches, vec![2..6]);
    }

    #[test]
    fn navigation_wraps() {
        let rope = Rope::from_str("x x x");
        let mut search = BufferSearch::default();
        search.set_query(&rope, "x", true);
        assert_eq!(search.current_match(), Some(0..1));
        assert_eq!(search.next_match(), Some(2..3));
        assert_eq!(search.next_match(), Some(4..5));
        assert_eq!(search.next_match(), Some(0..1));
        assert_eq!(search.previous_match(), Some(4..5));
    }
}
