# Text Editing Engine Specification (`EDITOR_ENGINE.md`)

## 1. Buffer Architecture (`ropey::Rope`)

Each open file is backed by a `Document` instance (`src/editor/document.rs`) built around `ropey::Rope`:

```rust
pub struct Document {
    pub id: BufferId,
    pub path: Option<PathBuf>,
    pub rope: Rope,
    pub version: u64,
    pub dirty: bool,
    pub encoding: TextEncoding,      // UTF-8, UTF-8-BOM, UTF-16LE, UTF-16BE
    pub line_ending: LineEnding,     // LF (\n) or CRLF (\r\n)
    pub language_id: Option<String>, // "rust", "typescript", "python", etc.
    pub cursors: CursorSet,
    pub history: UndoHistory,
    pub syntax: Option<SyntaxState>,
    pub folds: FoldMap,
    pub diagnostics: Vec<EditorDiagnostic>,
    pub git_diff: GutterDiff,
}
```

### Complexity Guarantees

| Operation | Time Complexity | Typical Latency (10 MB file) |
|-----------|-----------------|------------------------------|
| Insert / Delete at cursor | $O(\log N)$ | $< 1.5\ \mu\text{s}$ |
| Multi-cursor batch edit ($K$ cursors) | $O(K \log N)$ | $< 15\ \mu\text{s}$ ($K=20$) |
| Line index $\leftrightarrow$ Byte/Char index | $O(\log N)$ | $< 0.4\ \mu\text{s}$ |
| Snapshot clone for background thread (`rope.clone()`) | **$O(1)$** | $< 20\ \text{ns}$ (atomic refcount) |
| Viewport slice (`lines_at(row).take(height)`) | $O(\log N + H)$ | $< 5\ \mu\text{s}$ |

---

## 2. Multi-Cursor & Selection Model

Inspired by Helix and Kakoune, every cursor is a unified `SelectionRange` consisting of an `anchor` char index and a `head` (active cursor) char index, plus a `preferred_column` (visual display column) preserved across vertical movements:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub anchor: usize,               // Char index in Rope
    pub head: usize,                 // Active caret char index in Rope
    pub preferred_col: Option<usize> // Visual column in terminal cells
}

pub struct CursorSet {
    cursors: SmallVec<[Cursor; 1]>,  // Inline allocation for single-cursor fast path!
    primary_index: usize,
}
```

### Multi-Cursor Invariants
1. **Zero Heap Allocation for Single Cursor**: `SmallVec<[Cursor; 1]>` stores the common 1-cursor case directly on the stack/struct inline.
2. **Sorted & Non-Overlapping Normalization**: After every movement or edit transaction, `CursorSet::normalize()` sorts cursors by `(min(anchor, head), max(anchor, head))` and merges overlapping or adjacent selections automatically.
3. **Reverse-Order Edit Application (`EditTransaction`)**:
   - When $K$ cursors insert or delete text simultaneously, edits are sorted in **descending char-offset order** (from end of file to start of file) or applied with cumulative offset deltas so earlier edits never invalidate the offsets of later cursors.
   - Simultaneously emits exact `tree_sitter::InputEdit` and `lsp_types::TextDocumentContentChangeEvent` records.

---

## 3. Undo / Redo Transaction Tree (`src/editor/history.rs`)

Instead of cloning full buffers on every keystroke, `UndoHistory` stores compact `EditTransaction` nodes combined with $O(1)$ `Rope` structural sharing checkpoints:

```rust
pub struct HistoryNode {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub timestamp: Instant,
    pub before_rope: Rope,          // O(1) structural Arc snapshot!
    pub after_rope: Rope,           // O(1) structural Arc snapshot!
    pub before_cursors: CursorSet,
    pub after_cursors: CursorSet,
}
```

- **Why `Rope` snapshots in `HistoryNode` are memory-optimal**: Because `ropey::Rope` is a persistent B-tree of `Arc` nodes with 1 KB leaf chunks, storing `before_rope` and `after_rope` shares 99.9% of unchanged tree nodes in memory! A 1,000-step undo history on a 50 MB file consumes less than 1.5 MB of delta leaf nodes while giving **instant $O(1)$ undo and redo restoration**.
- **Keystroke Coalescing**: Consecutive character insertions within 350 ms without cursor jumps are coalesced into a single undo transaction; word boundaries, newlines, pastes, or mode transitions (`Insert` $\rightarrow$ `Normal`) seal the transaction boundary.

---

## 4. Incremental Syntax Highlighting (`tree-sitter` + `syntect`)

### Tree-sitter Pipeline (`src/editor/syntax.rs`)
1. On every `EditTransaction`, the byte range and `(row, column)` coordinates are converted into a `tree_sitter::InputEdit`.
2. `tree.edit(&input_edit)` updates the existing AST in microseconds.
3. For small files ($< 64\text{ KB}$), `parser.parse_with(..., Some(&old_tree))` runs inline in $< 0.4\text{ ms}$.
4. For large files ($> 64\text{ KB}$), an $O(1)$ `rope.clone()` and the `Tree` are dispatched to a background worker thread, which posts `AppEvent::SyntaxUpdated { buffer_id, version, tree, folds }` back to the main loop.
5. **Viewport-Scoped Query Execution**: `QueryCursor::set_byte_range(viewport_start_byte..viewport_end_byte)` ensures `highlights.scm` only evaluates AST nodes visible on screen.

### Code Folding & Bracket Matching
- **Code Folding (`FoldMap`)**: Extracted from Tree-sitter `folds.scm` queries (functions, structs, impl blocks, imports, markdown headings) plus indentation-based fallback. Folded regions map `document_line <-> visual_row` in $O(\log F)$ where $F$ is the number of active folds.
- **Bracket Matching**: Uses Tree-sitter node boundaries (or bounded 2,000-char scan fallback) to highlight matching `()`, `[]`, `{}` pairs and support `%` / `mm` jump-to-matching-bracket.
- **Smart Auto-Indentation**: Evaluates `indents.scm` on newline insertion, preserving tab/space style (`expand_tab`, `tab_width`) detected from workspace config or buffer heuristics.

---

## 5. Buffer-Local Search & Replace

- **Incremental Search (`/`, `?`, `Ctrl+F`)**:
  - Uses `grep-regex` / `regex-automata` streaming over `RopeChunks` without allocating a contiguous `String` for the file.
  - Highlights all visible matches in the viewport immediately while computing total match counts asynchronously.
- **Interactive Replace (`Ctrl+H` / `:%s/pat/rep/gc`)**:
  - Previews replacement inline and applies all accepted replacements as a single atomic `EditTransaction` (undoable with one `u` press).
