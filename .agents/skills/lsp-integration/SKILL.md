---
name: lsp-integration
description: Build and extend the Language Server Protocol (LSP 3.17) client, Content-Length JSON-RPC transport, UTF-8/UTF-16 position encoding translation, incremental document synchronization, diagnostics, completion, hover, goto-definition, and rename in src/lsp/.
---

# LSP Integration Skill (`lsp-integration`)

## 1. Purpose
Implement a non-blocking, multi-server Language Server Protocol (LSP 3.17) client in `src/lsp/` capable of routing intelligence requests (`hover`, `completion`, `definition`, `references`, `codeAction`, `rename`, `formatting`) and asynchronous diagnostics across multiple concurrent language servers.

## 2. Relevant Architecture
- **`LspManager` (`src/lsp/mod.rs`)**: Indexes active `LspClient` instances by `(WorkspaceRoot, ServerName)` and spawns servers lazily when matching documents open.
- **`LspFrameCodec` (`src/lsp/transport.rs`)**: Parses and encodes `Content-Length: <bytes>\r\n\r\n<json>` frames over `ChildStdout` / `ChildStdin`.
- **`PositionConverter` (`src/lsp/position.rs`)**: Translates between `ropey::Rope` `(line, char)` coordinates and the server's negotiated `PositionEncodingKind` (`utf-8`, `utf-16`, or `utf-32`).
- **`DiagnosticStore` (`src/lsp/diagnostics.rs`)**: Stores per-URI diagnostic lists and maps them to open `BufferId`s.

## 3. Implementation Guidelines
1. **Lazy On-Demand Spawning**:
   - Never block editor startup waiting for `rust-analyzer` or `pyright` to initialize.
   - Queue `didOpen` notifications while the server is in `Initializing` state and flush them immediately after receiving `InitializeResult` and sending `initialized`.
2. **Position Encoding Negotiation (`utf-8` vs `utf-16`)**:
   - Advertise `general.position_encodings = ["utf-8", "utf-16"]` in `InitializeParams`.
   - Store the negotiated encoding from `InitializeResult.capabilities.position_encoding` (defaulting to `utf-16` per LSP 3.17 spec if omitted).
   - Use `rope.char_to_utf16_cu()` / `rope.utf16_cu_to_char()` when `utf-16` is active so non-ASCII characters never shift column offsets.
3. **Incremental Document Sync (`didChange`)**:
   - When the server supports `TextDocumentSyncKind::INCREMENTAL`, emit exact `TextDocumentContentChangeEvent` ranges computed during `Document::apply_transaction`.
4. **URI Normalization**:
   - Convert `std::path::Path` to RFC 8089 `file://` `lsp_types::Url` (`Url::from_file_path`), stripping Windows `\\?\` UNC prefixes beforehand via `src/platform/paths.rs`.

## 4. Code Conventions
- Zero `ratatui` or `crossterm` imports inside `src/lsp/`.
- Pipe `stdin`, `stdout`, and `stderr` (`Stdio::piped()`); route `stderr` lines to `AppEvent::LspLog`.

## 5. Dependencies
- `lsp-types`
- `tokio` (`process`, `io`, `sync`, `time`)
- `tokio-util` (`codec`)
- `serde`, `serde_json`
- `url`

## 6. Common Mistakes to Avoid
- **Mistake**: Sending raw UTF-8 byte offsets as `Position.character` to a server using default UTF-16 encoding on a line containing emoji or accented characters.
  - **Fix**: Always run coordinates through `PositionConverter::to_lsp_position(&rope, char_idx, encoding)`.
- **Mistake**: Sending `textDocument/didOpen` or `textDocument/completion` before the `initialize` -> `initialized` handshake completes.
  - **Fix**: Buffer document sync messages in `LspClient` until `ServerState::Ready`.
- **Mistake**: Leaving orphaned language server processes running on IDE exit.
  - **Fix**: Send `shutdown` request + `exit` notification and terminate via `platform::process::terminate_child`.

## 7. Testing Requirements
- Unit-test `LspFrameCodec` with fragmented TCP/pipe reads and back-to-back concatenated frames.
- Unit-test `PositionConverter` with ASCII, 2-byte (`é`), 3-byte (`漢`), and 4-byte (`🦀`) characters under both `UTF-8` and `UTF-16` modes.

## 8. Project Documentation References
- [`docs/LSP_INTEGRATION.md`](../../../docs/LSP_INTEGRATION.md)
- [`docs/CROSS_PLATFORM.md`](../../../docs/CROSS_PLATFORM.md)
