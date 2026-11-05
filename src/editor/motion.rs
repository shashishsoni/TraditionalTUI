//! Cursor motions over a rope buffer.

use ropey::Rope;

use super::cursor::{Cursor, CursorSet};
use super::text::{
    char_at_visual_col, first_non_blank, is_word_char, line_bounds, line_last_char,
    next_grapheme_boundary, prev_grapheme_boundary, visual_col,
};

/// Motion requested by the keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
    Up,
    Down,
    PageUp(usize),
    PageDown(usize),
    DocStart,
    DocEnd,
}

/// Apply `motion` to every cursor. When `extend` is true the anchor is kept,
/// producing a selection.
pub fn apply(rope: &Rope, cursors: &mut CursorSet, motion: Motion, extend: bool, tab_width: usize) {
    let len = rope.len_chars();
    let primary_index = cursors.primary_index();
    let mut primary_head = cursors.primary().head;
    let mut vertical_col: Option<usize> = None;

    for (index, cursor) in cursors.iter_mut().enumerate() {
        let target = match motion {
            Motion::Left => move_left(rope, cursor.head),
            Motion::Right => move_right(rope, cursor.head),
            Motion::WordForward => word_forward(rope, cursor.head),
            Motion::WordBackward => word_backward(rope, cursor.head),
            Motion::WordEnd => word_end(rope, cursor.head),
            Motion::LineStart => line_start(rope, cursor.head),
            Motion::LineEnd => line_last_char(rope, cursor.head),
            Motion::Up => move_vertical(rope, cursor, -1, tab_width, &mut vertical_col),
            Motion::Down => move_vertical(rope, cursor, 1, tab_width, &mut vertical_col),
            Motion::PageUp(rows) => {
                move_vertical(rope, cursor, -(rows as isize), tab_width, &mut vertical_col)
            }
            Motion::PageDown(rows) => {
                move_vertical(rope, cursor, rows as isize, tab_width, &mut vertical_col)
            }
            Motion::DocStart => 0,
            Motion::DocEnd => len,
        };

        cursor.head = target.min(len);
        if !extend {
            cursor.anchor = cursor.head;
        }
        cursor.preferred_col = if is_vertical(motion) {
            vertical_col
        } else {
            None
        };
        if index == primary_index {
            primary_head = cursor.head;
        }
    }

    cursors.normalize(len);
    if let Some(index) = cursors
        .as_slice()
        .iter()
        .position(|cursor| cursor.head == primary_head)
    {
        cursors.set_primary_index(index);
    }
}

fn is_vertical(motion: Motion) -> bool {
    matches!(
        motion,
        Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
    )
}

fn move_left(rope: &Rope, head: usize) -> usize {
    let boundary = prev_grapheme_boundary(rope, head);
    if boundary == head {
        if head == 0 {
            0
        } else {
            line_last_char(rope, head - 1)
        }
    } else {
        boundary
    }
}

fn move_right(rope: &Rope, head: usize) -> usize {
    let len = rope.len_chars();
    let boundary = next_grapheme_boundary(rope, head);
    if boundary == head {
        if head < len { head + 1 } else { len }
    } else {
        boundary
    }
}

fn word_forward(rope: &Rope, head: usize) -> usize {
    let len = rope.len_chars();
    let mut index = head;
    while index < len && is_word_char(rope.char(index)) {
        index += 1;
    }
    while index < len && !is_word_char(rope.char(index)) {
        index += 1;
    }
    index
}

fn word_backward(rope: &Rope, head: usize) -> usize {
    let mut index = head;
    while index > 0 && !is_word_char(rope.char(index - 1)) {
        index -= 1;
    }
    while index > 0 && is_word_char(rope.char(index - 1)) {
        index -= 1;
    }
    index
}

fn word_end(rope: &Rope, head: usize) -> usize {
    let len = rope.len_chars();
    let mut index = head;
    if index < len && is_word_char(rope.char(index)) {
        index += 1;
    }
    while index < len && !is_word_char(rope.char(index)) {
        index += 1;
    }
    while index < len && is_word_char(rope.char(index)) {
        index += 1;
    }
    if index > head { index - 1 } else { index }
}

fn line_start(rope: &Rope, head: usize) -> usize {
    let blank = first_non_blank(rope, head);
    let (start, _) = line_bounds(rope, head);
    if head == blank { start } else { blank }
}

fn move_vertical(
    rope: &Rope,
    cursor: &mut Cursor,
    delta: isize,
    tab_width: usize,
    vertical_col: &mut Option<usize>,
) -> usize {
    let col = cursor
        .preferred_col
        .unwrap_or_else(|| visual_col(rope, cursor.head, tab_width));
    *vertical_col = Some(col);

    let current_line = rope.char_to_line(cursor.head);
    let line_count = rope.len_lines();
    let target_line =
        (current_line as isize + delta).clamp(0, line_count.saturating_sub(1) as isize) as usize;
    if target_line == current_line {
        return cursor.head;
    }
    char_at_visual_col(rope, target_line, col, tab_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor_at(rope: &Rope, position: usize) -> CursorSet {
        CursorSet::at(position.min(rope.len_chars()))
    }

    #[test]
    fn word_motion_crosses_punctuation() {
        let rope = Rope::from_str("hello, world\n");
        let mut cursors = cursor_at(&rope, 0);
        apply(&rope, &mut cursors, Motion::WordForward, false, 4);
        assert_eq!(cursors.primary().head, 7);
    }

    #[test]
    fn vertical_motion_preserves_preferred_column() {
        let rope = Rope::from_str("abcdef\nxy\nabcdef\n");
        let mut cursors = cursor_at(&rope, 4);
        apply(&rope, &mut cursors, Motion::Down, false, 4);
        assert_eq!(cursors.primary().head, 8);
        apply(&rope, &mut cursors, Motion::Down, false, 4);
        assert_eq!(cursors.primary().head, 14);
    }

    #[test]
    fn line_start_toggles() {
        let rope = Rope::from_str("    hello\n");
        let mut cursors = cursor_at(&rope, 6);
        apply(&rope, &mut cursors, Motion::LineStart, false, 4);
        assert_eq!(cursors.primary().head, 4);
        apply(&rope, &mut cursors, Motion::LineStart, false, 4);
        assert_eq!(cursors.primary().head, 0);
    }

    #[test]
    fn extend_creates_selection() {
        let rope = Rope::from_str("hello\n");
        let mut cursors = cursor_at(&rope, 0);
        apply(&rope, &mut cursors, Motion::WordForward, true, 4);
        assert_eq!(cursors.primary().anchor, 0);
        assert_eq!(cursors.primary().head, 6);
    }

    #[test]
    fn primary_tracking_survives_normalization() {
        let rope = Rope::from_str("one two three\n");
        let mut cursors = CursorSet::from_positions([0, 8]);
        cursors.set_primary_index(1);
        apply(&rope, &mut cursors, Motion::WordForward, false, 4);
        assert_eq!(cursors.primary().head, 14);
    }
}
