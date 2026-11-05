//! Character geometry helpers shared by cursors, motions and rendering.
//!
//! All offsets in the editor engine are **character indices** into a
//! `ropey::Rope` (`0..=len_chars()`), never raw byte offsets.

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Inclusive line start and exclusive line end (excluding the trailing `\n`).
pub fn line_bounds(rope: &Rope, char_idx: usize) -> (usize, usize) {
    let len = rope.len_chars();
    let idx = char_idx.min(len);
    let line = rope.char_to_line(idx);
    let start = rope.line_to_char(line);
    let end = if line + 1 < rope.len_lines() {
        let next = rope.line_to_char(line + 1);
        // Exclude a single `\n` (and a preceding `\r` if the buffer still has one).
        let mut end = next.saturating_sub(1);
        if end > start && rope.char(end - 1) == '\r' {
            end -= 1;
        }
        end
    } else {
        len
    };
    (start, end.min(len))
}

/// Line text without its terminator.
pub fn line_text(rope: &Rope, line: usize) -> String {
    if line >= rope.len_lines() {
        return String::new();
    }
    let start = rope.line_to_char(line);
    let (_, end) = line_bounds(rope, start);
    rope.slice(start..end).to_string()
}

/// Number of characters in `line`, excluding the terminator.
pub fn line_char_len(rope: &Rope, line: usize) -> usize {
    if line >= rope.len_lines() {
        return 0;
    }
    let start = rope.line_to_char(line);
    let (_, end) = line_bounds(rope, start);
    end - start
}

/// Visual terminal column for a char offset, expanding tabs.
pub fn visual_col(rope: &Rope, char_idx: usize, tab_width: usize) -> usize {
    let (start, end) = line_bounds(rope, char_idx);
    let idx = char_idx.clamp(start, end);
    let mut col = 0;
    let mut offset = start;
    let text = rope.slice(start..idx).to_string();
    for grapheme in text.graphemes(true) {
        let _ = offset;
        col += grapheme_width(grapheme, col, tab_width);
        offset += grapheme.chars().count();
    }
    col
}

/// Char offset on `line` nearest to visual column `target`, expanding tabs.
pub fn char_at_visual_col(rope: &Rope, line: usize, target: usize, tab_width: usize) -> usize {
    if line >= rope.len_lines() {
        return rope.len_chars();
    }
    let start = rope.line_to_char(line);
    let (_, end) = line_bounds(rope, start);
    let text = rope.slice(start..end).to_string();
    let mut col = 0;
    let mut chars = 0;
    for grapheme in text.graphemes(true) {
        let width = grapheme_width(grapheme, col, tab_width);
        if col + width > target {
            return start + chars;
        }
        col += width;
        chars += grapheme.chars().count();
    }
    if end > start && target > col {
        prev_grapheme_boundary(rope, end)
    } else {
        end
    }
}

/// Width of a grapheme, expanding `\t` to the next tab stop.
pub fn grapheme_width(grapheme: &str, current_col: usize, tab_width: usize) -> usize {
    match grapheme {
        "\t" => {
            let width = tab_width.max(1);
            width - (current_col % width)
        }
        "\r" | "\n" => 0,
        _ => grapheme.width().max(1),
    }
}

/// Previous grapheme-cluster boundary from `char_idx` on the same line.
pub fn prev_grapheme_boundary(rope: &Rope, char_idx: usize) -> usize {
    let (start, end) = line_bounds(rope, char_idx);
    let idx = char_idx.clamp(start, end);
    if idx <= start {
        return start;
    }
    let text = rope.slice(start..idx).to_string();
    let byte_idx = text
        .grapheme_indices(true)
        .next_back()
        .map(|(offset, _)| offset)
        .unwrap_or(0);
    start + text[..byte_idx].chars().count()
}

/// Next grapheme-cluster boundary from `char_idx` on the same line.
pub fn next_grapheme_boundary(rope: &Rope, char_idx: usize) -> usize {
    let (start, end) = line_bounds(rope, char_idx);
    let idx = char_idx.clamp(start, end);
    if idx >= end {
        return end;
    }
    let text = rope.slice(idx..end).to_string();
    let byte_idx = text
        .grapheme_indices(true)
        .next()
        .map(|(offset, grapheme)| offset + grapheme.len())
        .unwrap_or(text.len());
    idx + text[..byte_idx].chars().count()
}

/// Word character classification used by `w`, `b`, `e` motions.
pub fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// First non-blank char on the line containing `char_idx`.
pub fn first_non_blank(rope: &Rope, char_idx: usize) -> usize {
    let (start, end) = line_bounds(rope, char_idx);
    let text = rope.slice(start..end).to_string();
    let mut chars = 0;
    for ch in text.chars() {
        if ch != ' ' && ch != '\t' {
            break;
        }
        chars += 1;
    }
    start + chars
}

/// Char offset of the last char on the line (line start for an empty line).
pub fn line_last_char(rope: &Rope, char_idx: usize) -> usize {
    let (start, end) = line_bounds(rope, char_idx);
    if end > start {
        prev_grapheme_boundary(rope, end)
    } else {
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_bounds_exclude_newline() {
        let rope = Rope::from_str("abc\ndef\n");
        assert_eq!(line_bounds(&rope, 1), (0, 3));
        assert_eq!(line_bounds(&rope, 5), (4, 7));
    }

    #[test]
    fn grapheme_motion_skips_combining_sequences() {
        let rope = Rope::from_str("a👨‍👩‍👧‍👦b");
        let after_emoji = next_grapheme_boundary(&rope, 1);
        assert_eq!(after_emoji, 8);
        assert_eq!(prev_grapheme_boundary(&rope, 8), 1);
    }

    #[test]
    fn visual_columns_expand_tabs_and_wide_chars() {
        let rope = Rope::from_str("\t漢x");
        assert_eq!(visual_col(&rope, 1, 4), 4);
        assert_eq!(visual_col(&rope, 2, 4), 6);
        assert_eq!(char_at_visual_col(&rope, 0, 5, 4), 1);
    }

    #[test]
    fn first_non_blank_and_last_char_are_consistent() {
        let rope = Rope::from_str("   hello\n");
        assert_eq!(first_non_blank(&rope, 0), 3);
        assert_eq!(line_last_char(&rope, 0), 7);
    }
}
