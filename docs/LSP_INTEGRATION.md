# Language Server Protocol (LSP) Integration (`LSP_INTEGRATION.md`)

## 1. Architecture Overview

`src/lsp/` implements a non-blocking, multiplexed **Language Server Protocol 3.17** client capable of running multiple language servers concurrently within a single workspace (for example, `rust-analyzer` for `.rs`, `vtsls`/`typescript-language-server` for `.ts`, `pyright` for `.py`, `taplo` for `.toml`).

```
+-----------------------------------------------------------------------+
|                         `LspManager` (`src/lsp/mod.rs`)               |
|  Routes requests by `(WorkspaceRoot, LanguageId)` -> `ServerInstance` |
+-------------------+-----------------------------------+---------------+
                    |                                   |
                    v                                   v
      +---------------------------+       +---------------------------+
      | `LspClient` (rust-analyzer)|       | `LspClient` (pyright)     |
      | - `tokio::process::Child` |       | - `tokio::process::Child` |
      | - `LspFrameCodec` (stdio) |       | - `LspFrameCodec` (stdio) |
      | - Pending Request Map     |       | - Pending Request Map     |
      | - PositionEncoding (UTF-8)|       | - PositionEncoding(UTF-16)|
      +---------------------------+       +---------------------------+
```

---

## 2. Transport & JSON-RPC Framing (`src/lsp/transport.rs`)

LSP uses HTTP-style header framing over `stdin` and `stdout`:

```http
Content-Length: 142\r\n
Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"textDocument/definition","params":{...}}
```

- **Codec Implementation**: Implemented via `tokio_util::codec::{Decoder, Encoder}` operating over `BufReader<ChildStdout>` and `BufWriter<ChildStdin>`.
- **Zero-Stdout Contamination**: `stderr` is piped to a background reader task that forwards server log lines to `AppEvent::LspLog { server_id, line }`, displayed in the Bottom Panel's `[LSP]` tab.

---

## 3. Server Lifecycle & Capability Negotiation

1. **Lazy On-Demand Startup**:
   - Language servers are **never** spawned during cold startup if no matching file is open.
   - When the user opens a file with `language_id = "rust"`, `LspManager` checks if `rust-analyzer` is running for the detected workspace root (`Cargo.toml` / `.git`); if not, it spawns the server in the background after the first editor frame is already rendered.
2. **Handshake (`initialize` $\rightarrow$ `initialized`)**:
   - Sends `InitializeParams` with `root_uri`, `workspace_folders`, and client capabilities:
     - `position_encodings = ["utf-8", "utf-16"]` (prefers `"utf-8"` when supported by `rust-analyzer` / `clangd`, avoiding UTF-16 conversion overhead!).
     - Incremental document sync (`TextDocumentSyncKind::INCREMENTAL`).
     - Completion snippets, hover markdown, signature help, code actions, rename, formatting, diagnostic pull & push (`publishDiagnostics`), and inlay hints.
3. **Graceful Shutdown**:
   - On IDE exit or workspace switch, sends `shutdown` request (with 2-second timeout) followed by `exit` notification before terminating the process.

---

## 4. UTF-8 / UTF-16 Offset Translation (`src/lsp/position.rs`)

LSP servers negotiate one of three position encodings in `InitializeResult.capabilities.position_encoding`:
- **`utf-8`**: Column offset is raw UTF-8 byte offset within the line.
- **`utf-16` (LSP Default)**: Column offset is UTF-16 code units.
- **`utf-32`**: Column offset is Unicode scalar values (`char` count).

`ropey::Rope` natively supports fast $O(\log N)$ conversions between char indices, byte indices, and UTF-16 code units (`rope.char_to_utf16_cu()` and `rope.utf16_cu_to_char()`), ensuring cursor positions on lines containing multi-byte UTF-8 or astral emoji (`🦀`) never drift between the editor and the language server.

---

## 5. Document Synchronization & Core Features

| Feature | LSP Method | TUI UX Mapping |
|---------|------------|----------------|
| **Document Open / Save / Close** | `textDocument/didOpen`, `didSave`, `didClose` | Automatic on buffer lifecycle events. |
| **Incremental Edits** | `textDocument/didChange` | Generated directly from `EditTransaction` ranges; falls back to full rope text if server only supports `TextDocumentSyncKind::FULL`. |
| **Diagnostics** | `textDocument/publishDiagnostics` | Stored in `DiagnosticStore`; renders inline virtual text / underline in editor, gutter icons (`E`, `W`, `I`, `H`), Status Bar summary (`E:0 W:2`), and populates Bottom Panel `[Problems]` table. |
| **Autocomplete** | `textDocument/completion` | Non-blocking popup anchored below cursor (`Ctrl+Space` or trigger characters `.`, `:`, `>`). |
| **Hover Docs** | `textDocument/hover` | Floating markdown overlay (`K` in Normal mode) with syntax-highlighted code blocks. |
| **Navigation** | `definition`, `Declaration`, `references`, `implementation` | `gd`, `gD`, `gr`, `gi` — jumps directly if 1 location, or opens quick-pick preview list if multiple. |
| **Code Actions & Rename** | `textDocument/codeAction`, `rename` | `<Space>c` opens Code Action menu; `<Space>r` opens inline symbol rename input and applies returned `WorkspaceEdit` across buffers atomically. |
| **Formatting** | `textDocument/formatting` | `:fmt` or format-on-save applies returned `TextEdit` list to `Document` while preserving cursor position. |
