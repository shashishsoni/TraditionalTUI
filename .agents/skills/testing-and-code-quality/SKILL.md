---
name: testing-and-code-quality
description: Enforce code quality standards, Clippy zero-stdout rules, unit tests, proptest property fuzzing, headless Ratatui TestBackend snapshot tests, and mock LSP/ACP protocol tests. Use when writing tests, fixing lints, or verifying changes before completion.
---

# Testing and Code Quality Skill (`testing-and-code-quality`)

## 1. Purpose
Maintain strict engineering hygiene across `TradinationalTUI (ttui)`: zero compiler/Clippy warnings, zero terminal stdout/stderr contamination, no silent stub implementations, and comprehensive automated test coverage across all 16 modules.

## 2. Relevant Architecture
- **Crate Lints (`Cargo.toml` / `src/lib.rs`)**: Enforces `clippy::print_stdout = "deny"`, `clippy::print_stderr = "deny"`, `clippy::dbg_macro = "deny"`, and `#![forbid(unsafe_code)]`.
- **Unit & Property Tests (`src/**/tests`)**: Co-located `#[cfg(test)]` modules + `proptest` suites for `editor` and `search`.
- **Headless TUI Tests (`src/ui/` & `tests/`)**: Uses `ratatui::backend::TestBackend` to render frames into an in-memory buffer and inspect rendered text, styles, and layout bounds.

## 3. Implementation Guidelines
1. **No Silent Placeholders**:
   - Never implement a command or protocol method as `{ Ok(()) }` if the underlying logic is not yet wired. Return a typed `Error::NotImplemented(...)` or update `state.status_message` explicitly so the user and tests know the exact state.
2. **Error Handling Discipline**:
   - Define domain error enums with `thiserror::Error`.
   - Avoid `.unwrap()` and `.expect()` in library code; propagate errors with `?` or recover gracefully.
3. **Headless Testability**:
   - Every feature added to `editor`, `workspace`, `explorer`, `search`, `commands`, `acp`, `providers`, or `ui` must be testable without a real TTY by driving `AppState` and `TestBackend`.
4. **Small, Focused Functions**:
   - Keep functions under ~60 lines and files under ~500 lines. Extract helper functions with descriptive names and doc comments on public traits/structs.

## 4. Mandatory Pre-Completion Verification Sequence
Always run all four commands before declaring any engineering task complete:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo nextest run --all-targets || cargo test --all-targets
cargo bench --no-run
```

## 5. Dependencies
- `proptest` (dev-dependency)
- `criterion` (dev-dependency)
- `tempfile` (dev-dependency)
- `thiserror`, `anyhow`, `tracing`

## 6. Common Mistakes to Avoid
- **Mistake**: Using `println!` in a test helper or module that triggers `clippy::print_stdout`.
  - **Fix**: Crate-level lints apply to `--all-targets`; use `tracing::debug!` or assertions instead of `println!`.
- **Mistake**: Writing tests that depend on external network access or a user's personal API key.
  - **Fix**: Always use local mock streams or loopback fixtures for `acp`, `providers`, and `lsp` tests.

## 7. Project Documentation References
- [`docs/TESTING.md`](../../../docs/TESTING.md)
- [`AGENTS.md`](../../../AGENTS.md)
