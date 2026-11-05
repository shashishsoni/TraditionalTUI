//! Persistent Undo/Redo history with `O(1)` rope snapshots.
//!
//! Each node stores `before`/`after` `ropey::Rope` values. Because a `Rope`
//! clone only bumps the reference count on the root B-tree node, a thousand
//! undo steps on a large buffer share almost all memory with the live buffer.

use std::time::{Duration, Instant};

use ropey::Rope;

use super::cursor::CursorSet;

/// How an edit originated, used for keystroke coalescing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    Replace,
    Paste,
    MultiCursor,
    Other,
}

impl EditKind {
    fn coalescible(self, other: Self) -> bool {
        matches!(
            (self, other),
            (EditKind::Insert, EditKind::Insert) | (EditKind::Delete, EditKind::Delete)
        )
    }
}

#[derive(Debug, Clone)]
struct HistoryNode {
    before_rope: Rope,
    before_cursors: CursorSet,
    after_rope: Rope,
    after_cursors: CursorSet,
    kind: EditKind,
    at: Instant,
}

/// Undo/redo stacks.
#[derive(Debug, Clone)]
pub struct UndoHistory {
    undo: Vec<HistoryNode>,
    redo: Vec<HistoryNode>,
    coalesce_window: Duration,
}

impl Default for UndoHistory {
    fn default() -> Self {
        Self::new(Duration::from_millis(350))
    }
}

impl UndoHistory {
    pub fn new(coalesce_window: Duration) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            coalesce_window,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Record a completed edit snapshot.
    pub fn record(
        &mut self,
        before_rope: Rope,
        before_cursors: CursorSet,
        after_rope: Rope,
        after_cursors: CursorSet,
        kind: EditKind,
    ) {
        self.redo.clear();
        let now = Instant::now();

        if let Some(last) = self.undo.last_mut() {
            let within_window = now.duration_since(last.at) <= self.coalesce_window;
            // A cursor jump, paste or explicit transaction boundary must seal
            // the previous edit so undo restores a meaningful group.
            let contiguous = last.after_cursors == before_cursors;
            if within_window && contiguous && last.kind.coalescible(kind) {
                last.after_rope = after_rope;
                last.after_cursors = after_cursors;
                last.at = now;
                return;
            }
        }

        self.undo.push(HistoryNode {
            before_rope,
            before_cursors,
            after_rope,
            after_cursors,
            kind,
            at: now,
        });
    }

    /// Undo the last transaction, returning the restored rope and cursors.
    pub fn undo(&mut self) -> Option<(Rope, CursorSet)> {
        let node = self.undo.pop()?;
        let restored = (node.before_rope.clone(), node.before_cursors.clone());
        self.redo.push(node);
        Some(restored)
    }

    /// Redo the last undone transaction.
    pub fn redo(&mut self) -> Option<(Rope, CursorSet)> {
        let node = self.redo.pop()?;
        let restored = (node.after_rope.clone(), node.after_cursors.clone());
        self.undo.push(node);
        Some(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(before: &str, after: &str) -> (Rope, CursorSet, Rope, CursorSet) {
        (
            Rope::from_str(before),
            CursorSet::at(before.chars().count()),
            Rope::from_str(after),
            CursorSet::at(after.chars().count()),
        )
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut history = UndoHistory::new(Duration::ZERO);
        let (before, before_cursors, after, after_cursors) = node("a", "ab");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );

        let (rope, _) = history.undo().expect("undo");
        assert_eq!(rope.to_string(), "a");
        let (rope, _) = history.redo().expect("redo");
        assert_eq!(rope.to_string(), "ab");
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut history = UndoHistory::new(Duration::ZERO);
        let (before, before_cursors, after, after_cursors) = node("a", "ab");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );
        history.undo();
        let (before, before_cursors, after, after_cursors) = node("a", "ax");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );
        assert!(!history.can_redo());
    }

    #[test]
    fn contiguous_insertions_coalesce() {
        let mut history = UndoHistory::new(Duration::from_secs(10));
        let (before, before_cursors, after, after_cursors) = node("a", "ab");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );
        let (before, before_cursors, after, after_cursors) = node("ab", "abc");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );
        assert_eq!(history.depth(), 1);
        let (rope, _) = history.undo().expect("undo");
        assert_eq!(rope.to_string(), "a");
    }

    #[test]
    fn mixed_kinds_do_not_coalesce() {
        let mut history = UndoHistory::new(Duration::from_secs(10));
        let (before, before_cursors, after, after_cursors) = node("a", "ab");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Insert,
        );
        let (before, before_cursors, after, after_cursors) = node("ab", "a");
        history.record(
            before,
            before_cursors,
            after,
            after_cursors,
            EditKind::Delete,
        );
        assert_eq!(history.depth(), 2);
    }
}
