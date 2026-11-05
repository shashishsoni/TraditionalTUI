# Security Architecture & Threat Model (`SECURITY.md`)

## 1. Threat Model Overview

A modern terminal IDE that integrates external Language Servers (LSP), AI Coding Agents (ACP), Bring Your Own Key (BYOK) LLM providers, and repository-local configurations faces five primary security surfaces:

1. **Untrusted Workspace Repositories (`.ttui/config.toml`)** attempting to execute arbitrary binaries via custom LSP server or ACP agent commands.
2. **Autonomous ACP Agents** attempting unauthorized file reads/writes outside the workspace (`../../etc/passwd`, `~/.ssh/id_ed25519`) or destructive shell execution (`rm -rf`, credential exfiltration).
3. **API Key & Credential Exposure** in config files, crash dumps, debug logs, or environment inheritance into untrusted child processes.
4. **Prompt Injection via Repository Content or Tool Outputs** attempting to auto-approve ACP tool permissions.
5. **Terminal Escape Sequence Injection (ANSI Bombs)** in untrusted files or log streams attempting OSC title/clipboard hijacking.

---

## 2. Workspace Trust Model (`src/workspace/trust.rs`)

When `ttui` opens a directory containing a workspace-local `.ttui/config.toml`:
- **Untrusted Default (Safe Mode)**:
  - Allowed: Editor settings (`tab_width`, `expand_tab`, `line_numbers`, `theme`), file ignore patterns.
  - **Blocked until Trusted**: Workspace-defined custom `lsp.servers.<name>.command`, custom `acp.agents.<name>.command`, and automatic startup tasks.
- **Explicit Trust Grant**:
  - The user is prompted via a dedicated Workspace Trust banner (`:workspace trust`).
  - Trusted workspace canonical paths and their `.ttui/config.toml` SHA-256 hashes are persisted in the user state file (`~/.local/state/ttui/trusted_workspaces.json`). Any modification to executable commands in `.ttui/config.toml` invalidates the hash and re-prompts the user.

---

## 3. ACP Agent Sandboxing & Permission Gatekeeper (`src/acp/gatekeeper.rs`)

Even though ACP assumes a cooperative agent, `ttui` enforces strict client-side guardrails on every bidirectional request coming from an ACP agent subprocess:

1. **Canonical Path Confinement (`fs/read_text_file` & `fs/write_text_file`)**:
   - Every path requested by an agent MUST be absolute (per ACP spec).
   - `gatekeeper` resolves symlinks (`canonicalize`) and verifies that the target path is inside the active `workspace_root` (or an explicitly user-added directory).
   - Access to sensitive dotfiles (`.env`, `.env.*`, `.git/config`, `.git/hooks/*`, `~/.ssh/*`, `~/.aws/*`) requires explicit per-operation user confirmation regardless of session auto-edit settings.
2. **Interactive Permission Approval (`session/request_permission`)**:
   - All agent tool executions that mutate files outside standard editor buffers or execute shell commands (`terminal/create`) pause until the user responds to the TUI Permission Card:
     - `Allow Once (y)`
     - `Allow for Session (a)` (scoped strictly to the tool kind/prefix)
     - `Reject (n)`
     - `Inspect Diff (d)`
   - **No Silent Auto-Approval**: `ttui` never auto-approves shell command execution by default unless the user explicitly toggles `ACP Mode: Full Auto (Yolo)` with a persistent red warning badge in the status bar.

---

## 4. Credential & Secret Protection (`src/providers/credentials.rs`)

1. **OS Keyring Storage Only**:
   - BYOK API keys are stored in the native OS secret manager (`libsecret`/KWallet on Linux, Keychain on macOS, Credential Manager on Windows) via the `keyring` crate.
2. **Memory Zeroization & Redaction**:
   - All secrets in memory use `secrecy::SecretString` (`Zeroize` on drop).
   - `tracing` log formatters never log HTTP `Authorization`, `x-api-key`, or `x-goog-api-key` headers.
3. **Subprocess Environment Scrubbing**:
   - When spawning LSP servers or integrated terminal shells, BYOK secrets loaded from the OS keyring are **never** injected into child process environment variables.
4. **TLS Enforcement**:
   - `reqwest` is compiled with `rustls-tls` (pure-Rust memory-safe TLS 1.2/1.3).
   - Non-HTTPS endpoints (`http://`) are rejected by default unless the host is a loopback address (`127.0.0.1`, `localhost`, `[::1]`) for local Ollama / `llama.cpp` servers.

---

## 5. Terminal & ANSI Sanitization

- Text rendered inside `EditorWidget`, `FileExplorer`, `AiSidebar`, and `Problems` strips or escapes raw ASCII control characters (`0x00..=0x1F` except `\t` and `\n`, plus `0x7F` DEL and `0x9B` CSI) before writing to `ratatui::buffer::Buffer` cells, preventing malicious source files or LLM responses from injecting raw ANSI escape sequences into the host terminal.
