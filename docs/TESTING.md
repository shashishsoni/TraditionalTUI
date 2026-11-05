# Testing, Verification & Quality Assurance (`TESTING.md`)

## 1. Testing Philosophy

Every module in `TradinationalTUI (ttui)` is designed for **deterministic, headless testability without requiring an interactive TTY**.

The test pyramid consists of four layers:
1. **Unit Tests (`#[cfg(test)]` alongside modules)**: Fast, isolated verification of data structures, parsers, state transitions, and protocol codecs.
2. **Property-Based Fuzzing (`proptest`)**: Randomized invariant testing for multi-cursor rope edits, Unicode grapheme motions, and undo/redo trees.
3. **Headless TUI Snapshot Tests (`ratatui::backend::TestBackend` + `insta`)**: Pixel-exact (cell-exact) verification of layout splits, status bars, editor viewports, and permission modals across terminal sizes (`80x24`, `120x40`, `200x60`).
4. **End-to-End Subprocess Mock Tests (`tests/`)**: Real stdio JSON-RPC integration tests against mock LSP servers and mock ACP agents.

---

## 2. Test Requirements by Subsystem

### 2.1 Text Editing Engine (`src/editor/`)
- **Property Tests (`proptest`)**:
  - Generating sequences of arbitrary random insertions, deletions, multi-cursor selections (including multi-byte UTF-8, CJK wide characters, and ZWJ emoji `👨‍👩‍👧‍👦`), verifying that:
    1. Cursor offsets remain valid char boundaries (`<= rope.len_chars()`).
    2. `CursorSet` remains strictly sorted and non-overlapping after `normalize()`.
    3. Applying $N$ edits followed by $N$ `undo()` calls restores the exact byte-for-byte original `Rope` and `CursorSet`.
    4. `undo()` followed by `redo()` produces the exact post-edit `Rope`.

### 2.2 Terminal UI & Layout (`src/ui/`)
- **Headless `TestBackend` Rendering**:
  - Instantiate `ratatui::Terminal::new(TestBackend::new(width, height))`.
  - Feed synthetic `AppState` states (e.g., open Rust file + active diagnostic + open ACP permission card) and assert buffer cell contents and responsive auto-collapse behavior at `70x24`, `100x30`, and `160x50`.

### 2.3 ACP Client & Gatekeeper (`src/acp/`)
- **Mock ACP Agent over `tokio::io::duplex` / Stdio**:
  - Test `initialize` negotiation (v1).
  - Test `session/new` and `session/load`.
  - Test streaming `session/update` notifications (`agent_message_chunk`, `tool_call`, `plan`).
  - Test `session/request_permission` blocking until user approves/rejects in `AppState`, verifying exact JSON-RPC response payload.
  - Test path traversal rejection (`fs/read_text_file` requesting `/etc/shadow` or `../../outside_workspace` returns JSON-RPC error `-32602` / permission denied).

### 2.4 BYOK Providers (`src/providers/`)
- **Local Mock SSE Server**:
  - Test OpenAI, Anthropic, Gemini, and Ollama stream parsers against recorded SSE chunk fixtures (including partial UTF-8 splits across TCP packets, rate-limit `429` errors, and mid-stream `CancellationToken` aborts).

### 2.5 LSP Client (`src/lsp/`)
- **Framing & Offset Tests**:
  - Verify `Content-Length` parser handles multiple messages in a single read buffer and partial headers split across reads.
  - Verify UTF-8 $\leftrightarrow$ UTF-16 position conversion accuracy on lines with ASCII, 2-byte, 3-byte, and 4-byte surrogate-pair characters.

---

## 3. Running the Test & Quality Suite

```bash
# Run all unit and integration tests with cargo-nextest (or cargo test)
cargo nextest run --all-targets

# Fallback standard cargo test runner
cargo test --all-targets

# Run strict Clippy lints (zero warnings policy)
cargo clippy --all-targets -- -D warnings

# Verify formatting
cargo fmt --all -- --check

# Run Criterion performance benchmarks
cargo bench --no-run
```

---

## 4. Code Quality Gates (Enforced in `Cargo.toml` and CI)

- **`#![forbid(unsafe_code)]`** in application and domain modules (any low-level OS call is encapsulated in audited crates `crossterm`, `portable-pty`, `keyring`).
- **Clippy Lints Enforced**:
  - `clippy::print_stdout = "deny"` (prevents corrupting TUI alternate screen)
  - `clippy::print_stderr = "deny"` (prevents corrupting TUI alternate screen)
  - `clippy::dbg_macro = "deny"`
  - `clippy::unwrap_used = "warn"` (production paths must use `?`, `Result`, or explicit fallback)
  - `clippy::todo = "warn"` (no silent `todo!()` panics in shipped features)
