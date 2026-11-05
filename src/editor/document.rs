//! Rope-backed document buffer.
//!
//! All cursor offsets are character indices. Line endings are normalized to
//! `\n` in memory and restored to the original style on save. Encoding and
//! line endings are detected on load and preserved across round-trips.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ropey::Rope;

use super::cursor::{Cursor, CursorSet};
use super::error::EditorError;
use super::fold::FoldMap;
use super::history::{EditKind, UndoHistory};
use super::motion::{self, Motion};
use super::search;
use super::syntax::{HighlightState, language_id_for_path};
use super::text::{line_bounds, line_text, next_grapheme_boundary, prev_grapheme_boundary};
use crate::platform::fs::atomic_write;

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

/// Stable identifier for an open buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(pub u64);

impl BufferId {
    pub fn next() -> Self {
        Self(NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// Detected file encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
}

/// Line ending style preserved on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }
}

/// Diagnostic severity kept alongside a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorDiagnostic {
    pub line: usize,
    pub column: usize,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMark {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GutterDiff {
    pub marks: Vec<(usize, GutterMark)>,
}

/// One open text buffer.
#[derive(Debug, Clone)]
pub struct Document {
    pub id: BufferId,
    pub path: Option<PathBuf>,
    pub rope: Rope,
    pub version: u64,
    pub dirty: bool,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub language_id: String,
    pub cursors: CursorSet,
    pub history: UndoHistory,
    pub folds: FoldMap,
    pub diagnostics: Vec<EditorDiagnostic>,
    pub git_diff: GutterDiff,
    pub highlight_state: HighlightState,
    pub top_line: usize,
    pub left_col: usize,
}

impl Default for Document {
    fn default() -> Self {
        Self::scratch()
    }
}

impl Document {
    /// A new empty unsaved buffer.
    pub fn scratch() -> Self {
        Self {
            id: BufferId::next(),
            path: None,
            rope: Rope::new(),
            version: 1,
            dirty: false,
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::Lf,
            language_id: "plaintext".to_string(),
            cursors: CursorSet::at(0),
            history: UndoHistory::default(),
            folds: FoldMap::default(),
            diagnostics: Vec::new(),
            git_diff: GutterDiff::default(),
            highlight_state: HighlightState::default(),
            top_line: 0,
            left_col: 0,
        }
    }

    /// A buffer initialized from in-memory text.
    pub fn from_text(text: impl Into<String>) -> Self {
        let mut document = Self::scratch();
        document.rope = Rope::from_str(&text.into());
        document.version = 1;
        document.refresh_folds();
        document
    }

    /// Open a file from disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EditorError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        Self::from_bytes(Some(path.to_path_buf()), &bytes)
    }

    /// Decode a buffer from raw bytes, detecting BOM and line endings.
    pub fn from_bytes(path: Option<PathBuf>, bytes: &[u8]) -> Result<Self, EditorError> {
        let (decoded, encoding) = decode(bytes)?;
        let line_ending = if decoded.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
        let language_id = path
            .as_deref()
            .map(language_id_for_path)
            .unwrap_or("plaintext")
            .to_string();

        let mut document = Self::scratch();
        document.path = path;
        document.rope = Rope::from_str(&normalized);
        document.encoding = encoding;
        document.line_ending = line_ending;
        document.language_id = language_id;
        document.dirty = false;
        document.refresh_folds();
        Ok(document)
    }

    /// Save to the buffer's path.
    pub fn save(&mut self) -> Result<(), EditorError> {
        let path = self.path.clone().ok_or(EditorError::NoPath)?;
        self.save_to(&path)
    }

    /// Save to a new path, updating language detection.
    pub fn save_to(&mut self, path: &Path) -> Result<(), EditorError> {
        atomic_write(path, &self.encode())?;
        self.path = Some(path.to_path_buf());
        self.language_id = language_id_for_path(path).to_string();
        self.dirty = false;
        Ok(())
    }

    /// Encode the buffer for saving, restoring line endings and BOM.
    pub fn encode(&self) -> Vec<u8> {
        let text = if self.line_ending == LineEnding::Crlf {
            self.rope.to_string().replace('\n', "\r\n")
        } else {
            self.rope.to_string()
        };
        match self.encoding {
            TextEncoding::Utf8 => text.into_bytes(),
            TextEncoding::Utf8Bom => {
                let mut bytes = vec![0xEF, 0xBB, 0xBF];
                bytes.extend_from_slice(text.as_bytes());
                bytes
            }
            TextEncoding::Utf16Le => {
                let mut bytes = vec![0xFF, 0xFE];
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_le_bytes());
                }
                bytes
            }
            TextEncoding::Utf16Be => {
                let mut bytes = vec![0xFE, 0xFF];
                for unit in text.encode_utf16() {
                    bytes.extend_from_slice(&unit.to_be_bytes());
                }
                bytes
            }
        }
    }

    /// Superseding the buffer on disc (e.g. external reload).
    pub fn reload(&mut self) -> Result<(), EditorError> {
        let path = self.path.clone().ok_or(EditorError::NoPath)?;
        let replacement = Self::open(&path)?;
        let id = self.id;
        *self = replacement;
        self.id = id;
        Ok(())
    }

    pub fn char_count(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_text_at(&self, line: usize) -> String {
        line_text(&self.rope, line)
    }

    pub fn is_large_file(&self, threshold_mb: u64) -> bool {
        self.rope.len_bytes() as u64 > threshold_mb.max(1) * 1024 * 1024
    }

    /// Insert `text` at every cursor, replacing selections.
    pub fn insert_text(&mut self, text: &str, kind: EditKind) -> Result<(), EditorError> {
        let edits: Vec<(usize, usize, String)> = self
            .cursors
            .iter()
            .map(|cursor| (cursor.start(), cursor.end(), text.to_string()))
            .collect();
        self.apply_edits(edits, self.cursors.primary_index(), kind)
    }

    /// Replace arbitrary ranges with text as one undo transaction.
    pub fn replace_ranges(
        &mut self,
        ranges: &[(usize, usize, String)],
        kind: EditKind,
    ) -> Result<(), EditorError> {
        self.apply_edits(ranges.to_vec(), 0, kind)
    }

    /// Delete one grapheme before every cursor (or selected range).
    pub fn backspace(&mut self) -> Result<(), EditorError> {
        let mut edits = Vec::new();
        for cursor in self.cursors.iter() {
            if cursor.is_selection() {
                edits.push((cursor.start(), cursor.end(), String::new()));
                continue;
            }
            let head = cursor.head.min(self.rope.len_chars());
            let line_start = line_bounds(&self.rope, head).0;
            if head > line_start {
                edits.push((
                    prev_grapheme_boundary(&self.rope, head),
                    head,
                    String::new(),
                ));
            } else if head > 0 {
                edits.push((head - 1, head, String::new()));
            }
        }
        let primary = self.cursors.primary_index();
        self.apply_edits(edits, primary, EditKind::Delete)
    }

    /// Delete one grapheme after every cursor (or selected range).
    pub fn delete_forward(&mut self) -> Result<(), EditorError> {
        let len = self.rope.len_chars();
        let mut edits = Vec::new();
        for cursor in self.cursors.iter() {
            if cursor.is_selection() {
                edits.push((cursor.start(), cursor.end(), String::new()));
                continue;
            }
            let head = cursor.head.min(len);
            let (_, line_end) = line_bounds(&self.rope, head);
            if head < line_end {
                edits.push((
                    head,
                    next_grapheme_boundary(&self.rope, head),
                    String::new(),
                ));
            } else if head < len {
                edits.push((head, head + 1, String::new()));
            }
        }
        let primary = self.cursors.primary_index();
        self.apply_edits(edits, primary, EditKind::Delete)
    }

    /// Delete every cursor's line including its terminator.
    pub fn delete_lines(&mut self) -> Result<(), EditorError> {
        let len = self.rope.len_chars();
        let mut edits = Vec::new();
        let mut positions = Vec::new();
        for cursor in self.cursors.iter() {
            let line = self.rope.char_to_line(cursor.head.min(len));
            let start = self.rope.line_to_char(line);
            let end = if line + 1 < self.rope.len_lines() {
                self.rope.line_to_char(line + 1)
            } else if start > 0 {
                start - 1
            } else {
                len
            };
            if start <= end {
                edits.push((start, end, String::new()));
            }
            positions.push(start);
        }
        self.cursors = CursorSet::from_positions(positions);
        self.apply_edits(edits, 0, EditKind::Delete)
    }

    /// Apply a motion to every cursor.
    pub fn apply_motion(&mut self, movement: Motion, extend: bool, tab_width: usize) {
        motion::apply(&self.rope, &mut self.cursors, movement, extend, tab_width);
    }

    pub fn set_cursor(&mut self, position: usize) {
        self.cursors.set_single(position.min(self.rope.len_chars()));
    }

    pub fn set_cursors(&mut self, positions: impl IntoIterator<Item = usize>) {
        let len = self.rope.len_chars();
        self.cursors = CursorSet::from_positions(positions.into_iter().map(|pos| pos.min(len)));
    }

    pub fn select_all(&mut self) {
        let len = self.rope.len_chars();
        self.cursors = CursorSet::from_cursor(Cursor::selection(0, len));
    }

    pub fn select_line(&mut self) {
        let head = self.cursors.primary().head.min(self.rope.len_chars());
        let line = self.rope.char_to_line(head);
        let start = self.rope.line_to_char(line);
        let end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        self.cursors = CursorSet::from_cursor(Cursor::selection(start, end));
    }

    /// Move to a zero-based line, first non-blank column.
    pub fn goto_line(&mut self, line: usize) {
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        let start = self.rope.line_to_char(line);
        self.cursors.set_single(start);
    }

    pub fn undo(&mut self) -> bool {
        let Some((rope, cursors)) = self.history.undo() else {
            return false;
        };
        self.rope = rope;
        self.cursors = cursors;
        self.version = self.version.wrapping_add(1);
        self.dirty = true;
        self.refresh_folds();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((rope, cursors)) = self.history.redo() else {
            return false;
        };
        self.rope = rope;
        self.cursors = cursors;
        self.version = self.version.wrapping_add(1);
        self.dirty = true;
        self.refresh_folds();
        true
    }

    /// Zero-based `(line, column)` of the primary cursor.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let head = self.cursors.primary().head.min(self.rope.len_chars());
        let line = self.rope.char_to_line(head);
        let line_start = self.rope.line_to_char(line);
        (line, head - line_start)
    }

    /// One-based position shown in the status bar.
    pub fn status_position(&self) -> (usize, usize) {
        let (line, column) = self.cursor_line_col();
        (line + 1, column + 1)
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("scratch")
            .to_string()
    }

    pub fn find(&self, needle: &str, case_sensitive: bool) -> Vec<std::ops::Range<usize>> {
        search::find_all(&self.rope, needle, case_sensitive)
    }

    /// Replace all occurrences, returning the replacement count.
    pub fn replace_all(&mut self, needle: &str, replacement: &str, case_sensitive: bool) -> usize {
        let matches = search::find_all(&self.rope, needle, case_sensitive);
        if matches.is_empty() {
            return 0;
        }
        let edits: Vec<(usize, usize, String)> = matches
            .iter()
            .map(|range| (range.start, range.end, replacement.to_string()))
            .collect();
        let count = edits.len();
        if self.apply_edits(edits, 0, EditKind::Replace).is_err() {
            return 0;
        }
        count
    }

    /// Scroll the document so the primary cursor stays within the viewport.
    pub fn ensure_cursor_visible(&mut self, height: usize, scroll_off: usize) {
        if height == 0 {
            return;
        }
        let (line, _) = self.cursor_line_col();
        let margin = scroll_off.min(height / 2);
        if line < self.top_line + margin {
            self.top_line = line.saturating_sub(margin);
        } else if line + margin >= self.top_line + height {
            self.top_line = line + margin + 1 - height;
        }
        let max_top = self.line_count().saturating_sub(1);
        self.top_line = self.top_line.min(max_top);
    }

    fn apply_edits(
        &mut self,
        edits: Vec<(usize, usize, String)>,
        primary_index: usize,
        kind: EditKind,
    ) -> Result<(), EditorError> {
        if edits.is_empty() {
            return Ok(());
        }
        let len = self.rope.len_chars();
        for &(start, end, _) in &edits {
            if start > end || end > len {
                return Err(EditorError::range(start, end, len));
            }
        }

        let before_rope = self.rope.clone();
        let before_cursors = self.cursors.clone();
        let mut order: Vec<usize> = (0..edits.len()).collect();
        order.sort_by(|&a, &b| edits[b].0.cmp(&edits[a].0));

        for index in order {
            let (start, end, text) = &edits[index];
            self.rope.remove(*start..*end);
            if !text.is_empty() {
                self.rope.insert(*start, text);
            }
        }

        let mut positions = Vec::with_capacity(edits.len());
        for (start, _end, text) in &edits {
            let shift: isize = edits
                .iter()
                .filter(|(other_start, _, _)| other_start < start)
                .map(|(other_start, other_end, other_text)| {
                    other_text.chars().count() as isize
                        - (*other_end as isize - *other_start as isize)
                })
                .sum();
            let new_head =
                (*start as isize + shift + text.chars().count() as isize).max(0) as usize;
            positions.push(new_head);
        }
        let primary_head = positions.get(primary_index).copied().unwrap_or(0);
        self.cursors = CursorSet::from_positions(positions);
        if let Some(index) = self
            .cursors
            .as_slice()
            .iter()
            .position(|cursor| cursor.head == primary_head)
        {
            self.cursors.set_primary_index(index);
        }

        self.version = self.version.wrapping_add(1);
        self.dirty = true;
        self.refresh_folds();
        self.history.record(
            before_rope,
            before_cursors,
            self.rope.clone(),
            self.cursors.clone(),
            kind,
        );
        Ok(())
    }

    fn refresh_folds(&mut self) {
        let folded: Vec<usize> = self
            .folds
            .ranges()
            .iter()
            .filter(|range| self.folds.is_folded(range.start))
            .map(|range| range.start)
            .collect();
        let mut folds = FoldMap::compute(&self.rope, 4);
        for start in folded {
            let _ = folds.toggle(start);
        }
        self.folds = folds;
    }
}

fn decode(bytes: &[u8]) -> Result<(String, TextEncoding), EditorError> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((decode_utf8(rest)?, TextEncoding::Utf8Bom));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((decode_utf16(rest, true)?, TextEncoding::Utf16Le));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((decode_utf16(rest, false)?, TextEncoding::Utf16Be));
    }
    Ok((decode_utf8(bytes)?, TextEncoding::Utf8))
}

fn decode_utf8(bytes: &[u8]) -> Result<String, EditorError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| EditorError::InvalidUtf8)
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> Result<String, EditorError> {
    if bytes.len() % 2 != 0 {
        return Err(EditorError::InvalidUtf16);
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16(&units).map_err(|_| EditorError::InvalidUtf16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_undo_restore_content() {
        let mut document = Document::from_text("abc");
        document.set_cursor(3);
        document
            .insert_text("def", EditKind::Insert)
            .expect("insert");
        assert_eq!(document.rope.to_string(), "abcdef");
        assert!(document.dirty);
        assert!(document.undo());
        assert_eq!(document.rope.to_string(), "abc");
        assert!(document.redo());
        assert_eq!(document.rope.to_string(), "abcdef");
    }

    #[test]
    fn multi_cursor_insert_applies_to_all_carets() {
        let mut document = Document::from_text("a\nb\nc\n");
        document.set_cursors([0, 2, 4]);
        document
            .insert_text("X", EditKind::MultiCursor)
            .expect("insert");
        assert_eq!(document.rope.to_string(), "Xa\nXb\nXc\n");
        assert_eq!(document.cursors.len(), 3);
    }

    #[test]
    fn selection_replacement_deletes_range() {
        let mut document = Document::from_text("hello world");
        document.cursors = CursorSet::from_cursor(Cursor::selection(0, 5));
        document
            .insert_text("bye", EditKind::Replace)
            .expect("replace");
        assert_eq!(document.rope.to_string(), "bye world");
        assert_eq!(document.cursors.primary().head, 3);
    }

    #[test]
    fn backspace_joins_lines_at_line_start() {
        let mut document = Document::from_text("ab\ncd");
        document.set_cursor(3);
        document.backspace().expect("backspace");
        assert_eq!(document.rope.to_string(), "abcd");
        assert_eq!(document.cursors.primary().head, 2);
    }

    #[test]
    fn delete_lines_removes_terminator() {
        let mut document = Document::from_text("one\ntwo\nthree");
        document.set_cursor(5);
        document.delete_lines().expect("delete");
        assert_eq!(document.rope.to_string(), "one\nthree");
    }

    #[test]
    fn crlf_is_detected_normalized_and_restored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("crlf.txt");
        std::fs::write(&path, b"one\r\ntwo\r\n").expect("write");
        let mut document = Document::open(&path).expect("open");
        assert_eq!(document.line_ending, LineEnding::Crlf);
        assert_eq!(document.rope.to_string(), "one\ntwo\n");
        document.insert_text("!", EditKind::Insert).expect("insert");
        document.save().expect("save");
        let raw = std::fs::read(&path).expect("read");
        assert_eq!(raw, b"!one\r\ntwo\r\n");
    }

    #[test]
    fn utf16_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("utf16.txt");
        let mut original = vec![0xFF, 0xFE];
        for unit in "héllo".encode_utf16() {
            original.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&path, &original).expect("write");

        let mut document = Document::open(&path).expect("open");
        assert_eq!(document.encoding, TextEncoding::Utf16Le);
        assert_eq!(document.rope.to_string(), "héllo");
        document.save().expect("save");
        assert_eq!(std::fs::read(&path).expect("read"), original);
    }

    #[test]
    fn scratch_buffers_have_unique_ids() {
        let first = Document::scratch();
        let second = Document::scratch();
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn status_position_is_one_based() {
        let mut document = Document::from_text("abc\ndef");
        document.set_cursor(6);
        assert_eq!(document.cursor_line_col(), (1, 2));
        assert_eq!(document.status_position(), (2, 3));
    }

    #[test]
    fn replace_all_reports_count_and_is_undoable() {
        let mut document = Document::from_text("cat cat cat");
        assert_eq!(document.replace_all("cat", "dog", true), 3);
        assert_eq!(document.rope.to_string(), "dog dog dog");
        assert!(document.undo());
        assert_eq!(document.rope.to_string(), "cat cat cat");
    }

    #[test]
    fn invalid_ranges_are_rejected() {
        let mut document = Document::from_text("abc");
        let error = document
            .replace_ranges(&[(1, 99, "x".into())], EditKind::Replace)
            .expect_err("must fail");
        assert!(matches!(error, EditorError::InvalidRange { .. }));
    }
}
