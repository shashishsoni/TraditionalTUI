//! UTF-8 / UTF-16 / UTF-32 LSP position translation.

use ropey::Rope;

use crate::editor::text::line_bounds;

/// Position encoding negotiated with the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

impl PositionEncoding {
    pub fn from_lsp(value: Option<&str>) -> Self {
        match value {
            Some("utf-8") => Self::Utf8,
            Some("utf-32") => Self::Utf32,
            _ => Self::Utf16,
        }
    }

    pub fn as_lsp(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
            Self::Utf32 => "utf-32",
        }
    }
}

/// Convert a character offset into a zero-based LSP `(line, character)`.
pub fn to_lsp_position(rope: &Rope, char_idx: usize, encoding: PositionEncoding) -> (u32, u32) {
    let len = rope.len_chars();
    let index = char_idx.min(len);
    let line = rope.char_to_line(index);
    let line_start = rope.line_to_char(line);
    let character = match encoding {
        PositionEncoding::Utf8 => rope.char_to_byte(index) - rope.char_to_byte(line_start),
        PositionEncoding::Utf16 => rope.char_to_utf16_cu(index) - rope.char_to_utf16_cu(line_start),
        PositionEncoding::Utf32 => index - line_start,
    };
    (line as u32, character as u32)
}

/// Convert a zero-based LSP position into a character offset.
pub fn from_lsp_position(
    rope: &Rope,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> usize {
    let line = (line as usize).min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_char(line);
    let (_, line_end) = line_bounds(rope, line_start);
    let target = character as usize;

    let char_index = match encoding {
        PositionEncoding::Utf8 => {
            let line_start_byte = rope.char_to_byte(line_start);
            let line_end_byte = rope.char_to_byte(line_end);
            let byte = (line_start_byte + target).min(line_end_byte);
            let byte = snap_to_char_boundary(rope, byte);
            rope.byte_to_char(byte)
        }
        PositionEncoding::Utf16 => {
            let line_start_utf16 = rope.char_to_utf16_cu(line_start);
            let line_end_utf16 = rope.char_to_utf16_cu(line_end);
            let unit = (line_start_utf16 + target).min(line_end_utf16);
            rope.utf16_cu_to_char(unit)
        }
        PositionEncoding::Utf32 => line_start + target,
    };
    char_index.clamp(line_start, line_end)
}

fn snap_to_char_boundary(rope: &Rope, byte: usize) -> usize {
    let mut byte = byte.min(rope.len_bytes());
    while byte > 0 && byte < rope.len_bytes() && rope.try_byte_to_char(byte).is_err() {
        byte -= 1;
    }
    if rope.try_byte_to_char(byte).is_ok() {
        byte
    } else {
        0
    }
}

// Re-export used by the encoder; kept here to avoid widening the text API.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_positions_are_identical_across_encodings() {
        let rope = Rope::from_str("ab\ncd\n");
        for encoding in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            assert_eq!(to_lsp_position(&rope, 4, encoding), (1, 1));
            assert_eq!(from_lsp_position(&rope, 1, 1, encoding), 4);
        }
    }

    #[test]
    fn multibyte_characters_shift_utf8_and_utf16_columns() {
        let rope = Rope::from_str("é漢🦀x");
        // `é` is 2 UTF-8 bytes, 1 UTF-16 unit, 1 scalar.
        assert_eq!(to_lsp_position(&rope, 1, PositionEncoding::Utf8), (0, 2));
        assert_eq!(to_lsp_position(&rope, 1, PositionEncoding::Utf16), (0, 1));
        // `🦀` is 4 UTF-8 bytes, 2 UTF-16 units.
        assert_eq!(to_lsp_position(&rope, 3, PositionEncoding::Utf8), (0, 9));
        assert_eq!(to_lsp_position(&rope, 3, PositionEncoding::Utf16), (0, 4));
    }

    #[test]
    fn round_trips_positions() {
        let rope = Rope::from_str("héllo\n漢字🦀\nworld");
        for encoding in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            for char_idx in [0usize, 1, 3, 6, 8, 11, rope.len_chars()] {
                let (line, character) = to_lsp_position(&rope, char_idx, encoding);
                assert_eq!(
                    from_lsp_position(&rope, line, character, encoding),
                    char_idx,
                    "encoding={encoding:?} idx={char_idx}"
                );
            }
        }
    }

    #[test]
    fn out_of_range_positions_clamp_to_line() {
        let rope = Rope::from_str("abc\ndef");
        assert_eq!(from_lsp_position(&rope, 0, 99, PositionEncoding::Utf32), 3);
        assert_eq!(from_lsp_position(&rope, 99, 0, PositionEncoding::Utf32), 4);
    }
}
