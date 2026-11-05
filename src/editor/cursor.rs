//! Multi-cursor and selection model.
//!
//! `CursorSet` keeps the common single-cursor case inline in a `SmallVec` and
//! enforces the invariant that cursors are sorted and non-overlapping after
//! [`CursorSet::normalize`].

use smallvec::SmallVec;

/// A single cursor plus its selection anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Fixed end of the selection.
    pub anchor: usize,
    /// Active caret position.
    pub head: usize,
    /// Visual column preserved across vertical motions.
    pub preferred_col: Option<usize>,
}

impl Cursor {
    /// A collapsed caret at `position`.
    pub fn at(position: usize) -> Self {
        Self {
            anchor: position,
            head: position,
            preferred_col: None,
        }
    }

    /// A selection from `anchor` to `head`.
    pub fn selection(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            preferred_col: None,
        }
    }

    /// Normalized half-open range covered by this cursor.
    pub fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// Selection start (the lower offset).
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// Selection end (the higher offset).
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_selection(&self) -> bool {
        self.anchor != self.head
    }

    /// Clamp both endpoints into `0..=len`.
    pub fn clamp(&mut self, len: usize) {
        self.anchor = self.anchor.min(len);
        self.head = self.head.min(len);
    }
}

/// Sorted, non-overlapping set of cursors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorSet {
    cursors: SmallVec<[Cursor; 1]>,
    primary: usize,
}

impl Default for CursorSet {
    fn default() -> Self {
        Self::at(0)
    }
}

impl CursorSet {
    pub fn at(position: usize) -> Self {
        let mut cursors = SmallVec::new();
        cursors.push(Cursor::at(position));
        Self {
            cursors,
            primary: 0,
        }
    }

    pub fn from_cursor(cursor: Cursor) -> Self {
        let mut cursors = SmallVec::new();
        cursors.push(cursor);
        Self {
            cursors,
            primary: 0,
        }
    }

    pub fn from_positions(positions: impl IntoIterator<Item = usize>) -> Self {
        let mut cursors: SmallVec<[Cursor; 1]> = positions.into_iter().map(Cursor::at).collect();
        if cursors.is_empty() {
            cursors.push(Cursor::at(0));
        }
        let mut set = Self {
            cursors,
            primary: 0,
        };
        set.normalize(usize::MAX);
        set
    }

    pub fn from_ranges(ranges: impl IntoIterator<Item = std::ops::Range<usize>>) -> Self {
        let mut cursors: SmallVec<[Cursor; 1]> = ranges
            .into_iter()
            .map(|range| Cursor::selection(range.start, range.end))
            .collect();
        if cursors.is_empty() {
            cursors.push(Cursor::at(0));
        }
        let mut set = Self {
            cursors,
            primary: 0,
        };
        set.normalize(usize::MAX);
        set
    }

    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    pub fn as_slice(&self) -> &[Cursor] {
        &self.cursors
    }

    pub fn iter(&self) -> impl Iterator<Item = &Cursor> {
        self.cursors.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Cursor> {
        self.cursors.iter_mut()
    }

    pub fn primary(&self) -> &Cursor {
        &self.cursors[self.primary.min(self.cursors.len().saturating_sub(1))]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Select the active cursor after a normalization pass.
    pub fn set_primary_index(&mut self, index: usize) {
        if index < self.cursors.len() {
            self.primary = index;
        }
    }

    pub fn primary_mut(&mut self) -> &mut Cursor {
        let index = self.primary.min(self.cursors.len().saturating_sub(1));
        &mut self.cursors[index]
    }

    /// Collapse every selection to its active head.
    pub fn collapse_to_heads(&mut self) {
        for cursor in self.cursors.iter_mut() {
            cursor.anchor = cursor.head;
            cursor.preferred_col = None;
        }
    }

    /// Replace the whole set with a single caret.
    pub fn set_single(&mut self, position: usize) {
        self.cursors.clear();
        self.cursors.push(Cursor::at(position));
        self.primary = 0;
    }

    /// Add a caret and normalize.
    pub fn push(&mut self, cursor: Cursor, len: usize) {
        self.cursors.push(cursor);
        self.normalize(len);
    }

    /// Clamp every cursor into range.
    pub fn clamp(&mut self, len: usize) {
        for cursor in self.cursors.iter_mut() {
            cursor.clamp(len);
        }
    }

    /// Sort and merge overlapping or touching selections.
    ///
    /// Any selection containing the previous primary head becomes the new
    /// primary, keeping the active caret stable across normalization.
    pub fn normalize(&mut self, len: usize) {
        if self.cursors.is_empty() {
            self.cursors.push(Cursor::at(0));
            self.primary = 0;
            return;
        }
        self.clamp(len);
        let primary_head = self.primary().head;

        self.cursors.sort_by(|a, b| {
            a.start()
                .cmp(&b.start())
                .then_with(|| a.end().cmp(&b.end()))
        });

        let mut merged: SmallVec<[Cursor; 1]> = SmallVec::new();
        for cursor in self.cursors.drain(..) {
            match merged.last_mut() {
                Some(previous) if cursor.start() <= previous.end() => {
                    previous.anchor = previous.start().min(cursor.start());
                    previous.head = previous.end().max(cursor.end());
                    previous.preferred_col = previous.preferred_col.or(cursor.preferred_col);
                }
                _ => merged.push(cursor),
            }
        }

        self.primary = merged
            .iter()
            .position(|cursor| cursor.start() <= primary_head && primary_head <= cursor.end())
            .unwrap_or(0);
        self.cursors = merged;
    }

    /// Range covered by every cursor, useful for copy operations.
    pub fn overall_range(&self) -> std::ops::Range<usize> {
        let start = self.cursors.iter().map(Cursor::start).min().unwrap_or(0);
        let end = self.cursors.iter().map(Cursor::end).max().unwrap_or(0);
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_cursor_is_inline() {
        let set = CursorSet::at(4);
        assert_eq!(set.len(), 1);
        assert_eq!(set.primary().head, 4);
    }

    #[test]
    fn normalize_sorts_and_merges_overlaps() {
        let mut set = CursorSet::from_ranges([10..12, 0..3, 2..5]);
        set.normalize(100);
        assert_eq!(set.len(), 2);
        assert_eq!(set.as_slice()[0], Cursor::selection(0, 5));
        assert_eq!(set.as_slice()[1], Cursor::selection(10, 12));
    }

    #[test]
    fn normalize_preserves_primary_inside_merged_selection() {
        let mut set = CursorSet::from_positions([2, 10]);
        set.primary = 0;
        set.push(Cursor::at(4), 100);
        set.normalize(100);
        assert_eq!(set.primary().head, 2);
    }

    #[test]
    fn clamp_handles_shrinking_buffers() {
        let mut set = CursorSet::from_positions([5, 50]);
        set.clamp(10);
        assert!(set.as_slice().iter().all(|cursor| cursor.head <= 10));
    }

    #[test]
    fn ranges_are_normalized() {
        let cursor = Cursor::selection(9, 3);
        assert_eq!(cursor.range(), 3..9);
        assert_eq!(cursor.start(), 3);
        assert_eq!(cursor.end(), 9);
    }
}
