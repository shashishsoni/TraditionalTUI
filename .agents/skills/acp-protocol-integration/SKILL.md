---
name: acp-protocol-integration
description: Implement and maintain the Agent Client Protocol (ACP v1/v2) client, agent discovery, subprocess lifecycle, streaming session updates, and interactive permission gatekeeper in src/acp/. Use when integrating external coding agents (OpenCode, Command Code, Cursor Agent CLI, Gemini CLI, Claude Agent, Codex).
---

# ACP Protocol Integration Skill (`acp-protocol-integration`)

## 1. Purpose
Implement a compliant, secure, first-class **Agent Client Protocol (ACP)** client in `src/acp/` using the official `agent-client-protocol` Rust SDK, supporting any ACP-compatible external coding agent over bidirectional stdio JSON-RPC 2.0.

## 2. Relevant Architecture
- **`src/acp/registry.rs`**: Discovers installed ACP agents and distinguishes **Native ACP Agents** (`opencode acp`, `command-code-acp`, `cursor-agent acp`, `gemini --acp`, `goose acp`) from **Adapter-Backed ACP Agents** (`claude-agent-acp`, `codex-acp`).
- **`src/acp/supervisor.rs`**: Spawns and monitors the agent subprocess with piped `stdin`, `stdout`, and `stderr` (draining `stderr` to an internal ring buffer).
- **`src/acp/session.rs`**: Drives the ACP JSON-RPC lifecycle (`initialize` -> `authenticate` -> `session/new` or `session/load` -> `session/prompt` -> `session/update` notifications -> `session/cancel`).
- **`src/acp/gatekeeper.rs`**: Enforces workspace path confinement and routes `session/request_permission`, `fs/read_text_file`, `fs/write_text_file`, and `terminal/*` requests through user approval and live editor buffers.

## 3. Implementation Guidelines
1. **Official Protocol Compliance**:
   - Use newline-delimited JSON-RPC 2.0 messages over stdio (never send JSON-RPC batch arrays; request IDs must be strings or integers, never `null`).
   - All file paths exchanged in ACP messages (`cwd`, `fs/read_text_file`, `fs/write_text_file`, embedded context resources) **MUST** be native OS absolute paths (`/home/user/...` or `C:\Users\...`), never `file://` URIs.
   - All line numbers in ACP payloads are **1-based**.
2. **In-Memory Dirty Buffer Coherence**:
   - When an ACP agent calls `fs/read_text_file`, check `Workspace` first: if the file is open and has unsaved edits in `Document.rope`, return the live rope contents so the agent always sees the user's latest code.
   - When an agent calls `fs/write_text_file` (after permission checks), update both disk and any open `Document` via an undoable `EditTransaction`.
3. **Interactive Permission Gatekeeper (`session/request_permission`)**:
   - Never silently auto-approve destructive tool calls or shell commands.
   - Emit `AppEvent::AcpPermissionRequested(PermissionPrompt)` and await the user's decision (`AllowOnce`, `AllowAlways`, `RejectOnce`, `RejectAlways`) via a `tokio::sync::oneshot` channel before replying to the agent's JSON-RPC request.
4. **Cancellation (`session/cancel`)**:
   - When the user presses `Esc` or `Ctrl+C` during an active turn, send the `session/cancel` notification and resolve any pending permission prompt as cancelled.

## 4. Code Conventions
- Keep `src/acp/` completely decoupled from `src/providers/` (BYOK HTTP LLM clients).
- Always pipe child `stderr` (`Stdio::piped()`) so agent debug output never writes to the host terminal.

## 5. Dependencies
- `agent-client-protocol` (`features = ["process", "stdio"]`)
- `tokio` (`process`, `io`, `sync`)
- `serde`, `serde_json`

## 6. Common Mistakes to Avoid
- **Mistake**: Passing `file:///path/to/file` URIs (LSP style) into ACP requests.
  - **Fix**: ACP requires raw native absolute paths (`/path/to/file`).
- **Mistake**: Allowing `fs/read_text_file` or `fs/write_text_file` outside the workspace root (`../../etc/passwd`).
  - **Fix**: Canonicalize paths in `gatekeeper.rs` and verify `path.starts_with(&workspace_root)`.
- **Mistake**: Inheriting parent `stderr` when spawning the agent subprocess.
  - **Fix**: Always set `.stderr(Stdio::piped())` and stream lines to `AppEvent::AcpStderr`.

## 7. Testing Requirements
- Write integration tests using an in-memory `tokio::io::duplex` or mock stdio ACP peer testing `initialize`, `session/new`, streaming `session/update` chunks, `session/request_permission` round-trips, and path-traversal rejection.

## 8. Project Documentation References
- [`docs/ACP_INTEGRATION.md`](../../../docs/ACP_INTEGRATION.md)
- [`docs/SECURITY.md`](../../../docs/SECURITY.md)
