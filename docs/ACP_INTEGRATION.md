# Agent Client Protocol (ACP) Integration Specification (`ACP_INTEGRATION.md`)

## 1. Overview & Architectural Role

The **Agent Client Protocol (ACP)** is the open standard (created by Zed Industries and the Agent Client Protocol community) that standardizes communication between code editors (**Clients**) and AI coding agents (**Agents**) over bidirectional **JSON-RPC 2.0** stdio streams.

In `TradinationalTUI (ttui)`, ACP is implemented as a **first-class subsystem (`src/acp/`)** using the official Rust SDK (`agent-client-protocol` crate) and is strictly separated from the direct BYOK LLM provider layer (`src/providers/`).

```
+-------------------------------------------------------------------------+
|                           `ttui` IDE (ACP Client)                       |
|                                                                         |
|  +--------------------+   +----------------------+   +---------------+  |
|  |   `acp::registry`  |   |   `acp::supervisor`  |   | `acp::session`|  |
|  | Discovers & configures | Spawns agent child   |   | JSON-RPC v1/v2|  |
|  | installed agents   |   | process over stdio   |   | state machine |  |
|  +--------------------+   +----------+-----------+   +-------+-------+  |
|                                      |                       |          |
|                           +----------v-----------------------v-------+  |
|                           |      `acp::gatekeeper` (Security)        |  |
|                           | Intercepts:                              |  |
|                           |  - `session/request_permission`          |  |
|                           |  - `fs/read_text_file` & `write_text_file`  |
|                           |  - `terminal/create`, `output`, `kill`   |  |
|                           +------------------+-----------------------+  |
+----------------------------------------------|--------------------------+
                                               | Newline-delimited JSON-RPC 2.0
                                               | (stdin / stdout pipes)
                                               v
+-------------------------------------------------------------------------+
|                    External ACP Coding Agent Subprocess                 |
|  - `opencode acp`               (Native ACP)                            |
|  - `command-code-acp` / `cmd acp` (Native ACP)                          |
|  - `cursor-agent acp`           (Native Cursor CLI ACP)                 |
|  - `gemini --acp`               (Native Gemini CLI ACP)                 |
|  - `claude-agent-acp`           (Official Zed Adapter for Claude SDK)   |
|  - `codex-acp`                  (Official ACP Adapter for Codex CLI)    |
+-------------------------------------------------------------------------+
```

---

## 2. Native ACP Support vs. Adapter-Based Support

Per project requirements, `ttui` explicitly distinguishes between agents that implement ACP natively in their CLI binary versus agents that require an external bridge/adapter binary:

| Agent Name | Integration Type | Executable & Arguments | Protocol Version | Discovery Mechanism | Notes |
|------------|------------------|------------------------|------------------|---------------------|-------|
| **OpenCode** | **Native ACP** | `opencode acp` | ACP v1 (v2 ready) | `PATH` (`opencode`) | Launches isolated OpenCode ACP server over stdio. |
| **Command Code** | **Native ACP** | `command-code-acp` or `cmd acp` | ACP v1 | `PATH` / `~/.local/bin/command-code-acp` | Verified present on host (`/home/shashbyte/.local/bin/command-code-acp`). |
| **Cursor (CLI Agent)** | **Native CLI ACP** | `cursor-agent acp` or `agent acp` | ACP v1 | `PATH` / `~/.local/bin/agent` | Verified present on host (`~/.local/share/cursor-agent/.../cursor-agent`). Note: Cursor GUI IDE is a client, whereas `cursor-agent acp` is Cursor's headless CLI agent speaking ACP. |
| **Gemini CLI** | **Native ACP** | `gemini --acp` | ACP v1 | `PATH` (`gemini`) | Native flag in Google's `gemini-cli`. |
| **Goose** | **Native ACP** | `goose acp` | ACP v1 | `PATH` (`goose`) | Native subcommand in Block's Goose CLI. |
| **Claude Code / Agent** | **Adapter ACP** | `claude-agent-acp` | ACP v1 | `PATH` / npm `@zed-industries/claude-agent-acp` | Uses Zed's official ACP adapter wrapping Anthropic's Claude Agent SDK. |
| **OpenAI Codex CLI** | **Adapter ACP** | `codex-acp` | ACP v1 | `PATH` (`codex-acp`) | Uses official `agentclientprotocol/codex-acp` adapter. |
| **Custom ACP Agent** | **User Configured** | Configured in `config.toml` | ACP v1 / v2 | Explicit `[acp.agents.<id>]` | Any executable speaking ACP over stdin/stdout. |

---

## 3. Official ACP Protocol Lifecycle & Message Flow

`ttui` implements the complete ACP specification across four phases:

### Phase 1: Subprocess Spawn & Transport Framing
- The IDE spawns the selected agent binary with working directory set to the workspace root, `stdin = Stdio::piped()`, `stdout = Stdio::piped()`, and `stderr = Stdio::piped()`.
- Transport frames are **newline-delimited JSON-RPC 2.0 messages** over `stdin`/`stdout` (no JSON-RPC batch arrays; request IDs are strictly integers or strings, never `null`).
- A background task drains `stderr` into the IDE's **ACP Trace Log** ring buffer so agent diagnostic output never corrupts the TUI screen.

### Phase 2: Capability Negotiation & Authentication (`initialize`)
1. **Client $\rightarrow$ Agent (`initialize`)**:
   - `protocolVersion`: Negotiates v1 (`1`) or v2 (when opted in).
   - `clientInfo`: `{ "name": "TradinationalTUI", "version": "0.1.0" }`
   - `clientCapabilities`:
     - `fs`: `{ "readTextFile": true, "writeTextFile": true }`
     - `terminal`: `true`
     - `elicitation`: `{ "form": true, "url": true }`
2. **Agent $\rightarrow$ Client (`InitializeResponse`)**:
   - Returns negotiated `protocolVersion`, `agentInfo`, `authMethods`, and `agentCapabilities` (`loadSession`, `promptCapabilities`, `mcpCapabilities`).
3. **Authentication (`authenticate` in v1 / `auth/login` in v2)**:
   - If the agent requires authentication and advertises `authMethods`, `ttui` prompts the user in the AI panel and invokes `authenticate` (or runs the advertised terminal login flow and reconnects).

### Phase 3: Session Setup (`session/new`, `session/load`, `session/resume`)
- **Create Session (`session/new`)**:
  - Sends `{ "cwd": "/abs/path/to/workspace", "mcpServers": [...] }`.
  - Receives `{ "sessionId": "...", "modes": { "currentModeId": "...", "availableModes": [...] }, "models": ... }`.
- **Restore Session (`session/load` in v1 / `session/resume` in v2)**:
  - Persists active `sessionId` per workspace in `.ttui/state.json`.
  - When reopening a workspace, if the agent advertises `loadSession` (v1) or `session.resume` (v2), `ttui` restores the conversation and replays history updates into the AI Sidebar.

### Phase 4: Prompt Turn, Streaming Updates, & Bidirectional Client Callbacks
1. **Send Prompt (`session/prompt`)**:
   - Sends user prompt blocks: text, `@file` embedded resource snippets (with 1-based line numbers and native absolute file paths), and active diagnostics.
2. **Receive Real-Time Streaming Notifications (`session/update`)**:
   - `agent_message_chunk`: Appends streamed markdown tokens to the active assistant message bubble.
   - `agent_thought_chunk`: Streams collapsible reasoning/thinking blocks in the TUI.
   - `tool_call` / `tool_call_update`: Renders live tool execution cards (`pending` $\rightarrow$ `in_progress` $\rightarrow$ `completed` / `failed`), including inline unified diffs (`ContentBlock::Diff`) and terminal output previews.
   - `plan`: Updates the visual checklist of agent steps at the top of the AI Sidebar.
   - `available_commands_update`: Populates agent slash commands (`/compact`, `/review`, `/plan`) into `ttui`'s Command Palette and prompt autocomplete.
   - `current_mode_update` / `config_option_update`: Synchronizes the mode/model badge in the TUI status bar.
3. **Handle Agent-to-Client Bidirectional Requests**:
   - **`session/request_permission`**:
     - Triggered before destructive edits or shell commands.
     - Displays an interactive permission card in the Right AI Sidebar (and optional floating modal badge) with the agent's `options` (e.g., `allow_once`, `allow_always`, `reject_once`, `reject_always`).
     - Supports keyboard approval (`y` = allow once, `a` = allow for session, `n` = reject, `d` = inspect full diff in editor split).
   - **`fs/read_text_file`**:
     - Validates that the requested path is absolute and within the workspace trust boundary.
     - **Dirty Buffer Coherence**: If the file is currently open and modified in `ttui`'s `Workspace`, serves the live in-memory `ropey::Rope` content (or saves if configured) so the agent sees unsaved user edits!
   - **`fs/write_text_file`**:
     - Validates workspace boundaries, applies the change through `Workspace` so open editor buffers update in real time with an undoable `EditTransaction`, and triggers Tree-sitter/LSP refresh.
   - **`terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`**:
     - Delegates to `src/terminal/` (`TerminalManager`), creating a visible, inspectable tab in the Bottom Terminal Panel labeled `[ACP: <cmd>]` so the user can watch agent test runs in real time.
4. **Cancellation (`session/cancel` & `$/cancel_request`)**:
   - Pressing `Esc` or `Ctrl+C` in the AI panel while a turn is active sends the `session/cancel` notification to the agent and cancels any pending permission prompt.

---

## 4. Error Handling, Reconnection & Supervision

- **Crash Recovery**: If the agent subprocess exits unexpectedly (non-zero status or broken pipe), `AcpSupervisor` marks the session disconnected, captures the last 100 lines of `stderr`, displays a `[Reconnect Agent (r)]` banner in the AI Sidebar, and attempts `session/load` upon restart.
- **Request Timeouts**: Initialization and session setup enforce configurable timeouts (default 15s); `session/prompt` turns remain open as long as `session/update` heartbeats or tool calls are progressing.
- **Path Normalization**: Enforces ACP's mandatory rule that all file paths over the wire are **native absolute OS paths** (`/home/...` on Unix, `C:\Users\...` on Windows—never `file://` URIs) and all line numbers are **1-based**.
