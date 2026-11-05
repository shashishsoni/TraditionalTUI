//! Indentation-based code folding.
//!
//! Fold ranges are derived from leading indentation: a line opens a fold when
//! the following line is more indented, and the fold extends through the last
//! line of that deeper block. This works for every language and stays cheap.

use std::collections::HashSet;

use ropey::Rope;

use super::text::line_text;

/// A fold spanning `start..=end` (inclusive document lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRange {
    pub start: usize,
    pub end: usize,
}

/// Fold ranges plus the set of currently collapsed fold starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldMap {
    ranges: Vec<FoldRange>,
    folded: HashSet<usize>,
}

impl FoldMap {
    /// Recompute fold ranges from buffer indentation.
    pub fn compute(rope: &Rope, tab_width: usize) -> Self {
        let line_count = rope.len_lines();
        if line_count > 20_000 {
            return Self::default();
        }
        let indent: Vec<usize> = (0..line_count)
            .map(|line| indent_width(&line_text(rope, line), tab_width))
            .collect();

        let mut ranges = Vec::new();
        for start in 0..line_count.saturating_sub(1) {
            let text = line_text(rope, start);
            if text.trim().is_empty() {
                continue;
            }
            let base = indent[start];
            if indent[start + 1] <= base {
                continue;
            }
            let mut end = start + 1;
            let mut last_content = start + 1;
            while end + 1 < line_count && indent[end + 1] > base {
                end += 1;
                if !line_text(rope, end).trim().is_empty() {
                    last_content = end;
                }
            }
            if last_content > start {
                ranges.push(FoldRange {
                    start,
                    end: last_content,
                });
            }
        }

        Self {
            ranges,
            folded: HashSet::new(),
        }
    }

    pub fn ranges(&self) -> &[FoldRange] {
        &self.ranges
    }

    /// Whether the fold opening at `line` is collapsed.
    pub fn is_folded(&self, line: usize) -> bool {
        self.folded.contains(&line)
    }

    /// Toggle the fold opening at `line`, returning the new collapsed state.
    pub fn toggle(&mut self, line: usize) -> bool {
        if !self.ranges.iter().any(|range| range.start == line) {
            return false;
        }
        if !self.folded.remove(&line) {
            self.folded.insert(line);
            true
        } else {
            false
        }
    }

    /// A document line is hidden when it lies strictly inside a collapsed fold.
    pub fn is_hidden(&self, line: usize) -> bool {
        self.ranges.iter().any(|range| {
            self.folded.contains(&range.start) && line > range.start && line <= range.end
        })
    }

    /// Expand `visual` rows into document lines, skipping folded bodies.
    pub fn visible_lines(&self, line_count: usize) -> Vec<usize> {
        (0..line_count)
            .filter(|line| !self.is_hidden(*line))
            .collect()
    }

    /// Number of document lines currently hidden.
    pub fn hidden_count(&self) -> usize {
        self.ranges
            .iter()
            .filter(|range| self.folded.contains(&range.start))
            .map(|range| range.end - range.start)
            .sum()
    }
}

fn indent_width(text: &str, tab_width: usize) -> usize {
    let mut width = 0;
    for ch in text.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += tab_width - (width % tab_width.max(1)),
            _ => break,
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_indented_blocks() {
        let rope = Rope::from_str("fn a() {\n    let x = 1;\n    let y = 2;\n}\nfn b() {}\n");
        let folds = FoldMap::compute(&rope, 4);
        assert!(
            folds
                .ranges()
                .iter()
                .any(|range| range.start == 0 && range.end == 2)
        );
    }

    #[test]
    fn toggle_hides_body_lines() {
        let rope = Rope::from_str("fn a() {\n    one\n    two\n}\n");
        let mut folds = FoldMap::compute(&rope, 4);
        assert!(!folds.is_hidden(1));
        assert!(folds.toggle(0));
        assert!(folds.is_folded(0));
        assert!(folds.is_hidden(1));
        assert!(!folds.is_hidden(0));
    }

    #[test]
    fn visible_lines_skip_folded_ranges() {
        let rope = Rope::from_str("a\n    b\n    c\nd");
        let mut folds = FoldMap::compute(&rope, 4);
        folds.toggle(0);
        assert_eq!(folds.visible_lines(rope.len_lines()), vec![0, 3]);
    }
}
