---
name: editor-engine-development
description: Implement and extend the UTF-8 rope buffer, multi-cursor engine, modal editing motions, undo/redo history tree, Tree-sitter incremental syntax highlighting, code folding, and buffer search in src/editor/. Use when working on text manipulation, cursors, selections, or syntax parsing.
---

# Editor Engine Development Skill (`editor-engine-development`)

## 1. Purpose
Maintain a rock-solid, sub-millisecond text editing engine in `src/editor/` powered by `ropey::Rope`, `SmallVec`-backed multi-cursors, persistent structural-sharing Undo/Redo history, and incremental `tree-sitter` AST analysis.

## 2. Relevant Architecture
- **`Document` (`src/editor/document.rs`)**: Encapsulates `ropey::Rope`, `CursorSet`, `UndoHistory`, `SyntaxState`, `FoldMap`, `TextEncoding`, and `LineEnding`.
- **`CursorSet` (`src/editor/cursor.rs`)**: Manages `SmallVec<[Cursor; 1]>` with `anchor`, `head`, and `preferred_col`, enforcing sorted, non-overlapping selection invariants.
- **`UndoHistory` (`src/editor/history.rs`)**: Stores $O(1)$ copy-on-write `Rope` checkpoints and `CursorSet` snapshots per transaction.
- **`SyntaxState` (`src/editor/syntax.rs`)**: Incremental `tree-sitter` parser + `syntect` fallback.

## 3. Implementation Guidelines
1. **Char vs. Byte vs. Line Indexing in `ropey::Rope`**:
   - Store cursor `anchor` and `head` as **character indices (`usize`)** within `0..=rope.len_chars()`.
   - Always clamp indices using `rope.len_chars()` and snap to grapheme cluster boundaries so cursors never land inside a CRLF `\r\n` pair or multi-codepoint emoji.
   - Convert to byte offsets via `rope.char_to_byte(char_idx)` when feeding `tree_sitter::InputEdit`.
2. **Multi-Cursor Edit Application**:
   - Normalize cursors (`CursorSet::normalize()`) before and after every edit transaction.
   - Apply multi-cursor replacements in **reverse offset order** (highest char index down to lowest) or track cumulative char delta so earlier edits do not shift subsequent cursor targets.
3. **$O(1)$ Undo/Redo Snapshots**:
   - Before applying an `EditTransaction`, capture `before_rope = self.rope.clone()` ($O(1)$ `Arc` clone) and `before_cursors = self.cursors.clone()`.
   - Coalesce sequential single-character insertions within 350 ms; start a new undo node on whitespace, newlines, deletions, pastes, or mode changes.
4. **Line Ending & Encoding Preservation**:
   - Detect `LineEnding::Lf` vs `LineEnding::Crlf` on load and preserve on save.

## 4. Code Conventions
- Zero `ratatui` or `crossterm` imports in `src/editor/`.
- Return `Result<..., EditorError>` for fallible operations; never `.unwrap()` out-of-bounds rope slices.

## 5. Dependencies
- `ropey`
- `smallvec`
- `unicode-segmentation`
- `unicode-width`
- `tree-sitter`
- `syntect`
- `imara-diff`

## 6. Common Mistakes to Avoid
- **Mistake**: Applying multi-cursor edits from index 0 upwards without adjusting subsequent cursor positions.
  - **Fix**: Apply edits from the last cursor down to the first cursor, then adjust all cursor offsets deterministically.
- **Mistake**: Calling `rope.to_string()` to search or highlight a buffer.
  - **Fix**: Use `RopeSlice::chunks()` with streaming `grep-regex` or Tree-sitter's callback `parse_with`.
- **Mistake**: Splitting `\r\n` by moving a cursor 1 `char` left from `\n`.
  - **Fix**: Move by extended grapheme clusters (`grapheme_indices(true)`).

## 7. Testing Requirements
- Add unit tests for every motion, edit, and fold operation.
- Run `proptest` randomized multi-cursor edit + undo/redo invariants (`cargo test --test editor_proptest` or unit `proptest!` blocks).

## 8. Project Documentation References
- [`docs/EDITOR_ENGINE.md`](../../../docs/EDITOR_ENGINE.md)
- [`docs/PERFORMANCE.md`](../../../docs/PERFORMANCE.md)
- [`docs/TESTING.md`](../../../docs/TESTING.md)
