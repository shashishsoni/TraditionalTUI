//! Asynchronous LSP 3.17 client over stdio JSON-RPC.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::codec::{FramedRead, FramedWrite};
use url::Url;

use super::diagnostics::{DiagnosticEntry, Severity};
use super::error::LspError;
use super::position::PositionEncoding;
use super::transport::LspFrameCodec;
use crate::platform::process::configure_tokio_command;

/// How much document content the server expects per change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncKind {
    None,
    Full,
    #[default]
    Incremental,
}

/// Server lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Starting,
    Ready,
    Stopping,
    Stopped,
}

/// Events emitted by a running client.
#[derive(Debug, Clone)]
pub enum LspEvent {
    StateChanged {
        server: String,
        state: ServerState,
    },
    Diagnostics {
        uri: String,
        entries: Vec<DiagnosticEntry>,
    },
    Log {
        server: String,
        line: String,
    },
    Exited {
        server: String,
        status: Option<String>,
    },
}

/// Configuration for one language server process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub languages: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

impl LspServerConfig {
    pub fn new(
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        languages: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args,
            languages,
            env: std::collections::BTreeMap::new(),
        }
    }

    pub fn supports(&self, language: &str) -> bool {
        self.languages
            .iter()
            .any(|candidate| candidate == language || candidate == "*")
    }
}

/// One live language server connection.
pub struct LspClient {
    name: String,
    child: Option<Child>,
    outgoing: mpsc::Sender<Value>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    next_id: AtomicI64,
    position_encoding: PositionEncoding,
    sync_kind: SyncKind,
    state: ServerState,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LspClient")
            .field("name", &self.name)
            .field("state", &self.state)
            .field("position_encoding", &self.position_encoding)
            .field("sync_kind", &self.sync_kind)
            .finish_non_exhaustive()
    }
}

impl LspClient {
    /// Spawn the configured server with piped stdio.
    pub async fn spawn(
        config: &LspServerConfig,
        root: &Path,
        events: mpsc::UnboundedSender<LspEvent>,
    ) -> Result<Self, LspError> {
        let mut command = tokio::process::Command::new(&config.command);
        command
            .args(&config.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(&config.env)
            // Never leak BYOK credentials into language server processes.
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GEMINI_API_KEY");
        configure_tokio_command(&mut command);

        let mut child = command.spawn().map_err(|source| LspError::Spawn {
            server: config.name.clone(),
            source,
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Protocol("server stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Protocol("server stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| LspError::Protocol("server stderr unavailable".into()))?;

        let (outgoing, outgoing_rx) = mpsc::channel(256);
        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(writer_task(stdin, outgoing_rx));
        tokio::spawn(reader_task(
            stdout,
            pending.clone(),
            events.clone(),
            config.name.clone(),
            outgoing.clone(),
        ));
        tokio::spawn(stderr_task(stderr, events, config.name.clone()));

        Ok(Self {
            name: config.name.clone(),
            child: Some(child),
            outgoing,
            pending,
            next_id: AtomicI64::new(1),
            position_encoding: PositionEncoding::Utf16,
            sync_kind: SyncKind::Incremental,
            state: ServerState::Starting,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> ServerState {
        self.state
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }

    pub fn sync_kind(&self) -> SyncKind {
        self.sync_kind
    }

    /// Perform the `initialize`/`initialized` handshake.
    pub async fn initialize(&mut self, root: &Path) -> Result<(), LspError> {
        let root_uri = path_to_uri(root)?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "TradinationalTUI",
                "version": crate::VERSION,
            },
            "rootUri": root_uri,
            "workspaceFolders": [{
                "uri": root_uri,
                "name": name,
            }],
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-8", "utf-16"],
                },
                "workspace": {
                    "workspaceFolders": true,
                },
                "textDocument": {
                    "synchronization": {
                        "didSave": true,
                        "dynamicRegistration": false,
                    },
                    "publishDiagnostics": {
                        "relatedInformation": false,
                    },
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                        },
                    },
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"],
                    },
                    "definition": {},
                    "references": {},
                    "rename": {},
                    "formatting": {},
                },
            },
        });
        let result = self.request("initialize", params).await?;
        let encoding = result
            .pointer("/capabilities/positionEncoding")
            .and_then(Value::as_str);
        self.position_encoding = PositionEncoding::from_lsp(encoding);
        self.sync_kind = parse_sync_kind(&result);
        self.notify("initialized", json!({})).await?;
        self.state = ServerState::Ready;
        Ok(())
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;

        let response = match tokio::time::timeout(Duration::from_secs(15), receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return Err(LspError::Closed),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(LspError::Timeout);
            }
        };
        if let Some(error) = response.get("error") {
            return Err(LspError::Protocol(error.to_string()));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    pub async fn did_open(
        &self,
        path: &Path,
        language_id: &str,
        version: u64,
        text: &str,
    ) -> Result<(), LspError> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": path_to_uri(path)?,
                    "languageId": language_id,
                    "version": version,
                    "text": text,
                }
            }),
        )
        .await
    }

    /// Send a full-document change. Omitting `range.lenght` is valid for both
    /// FULL and INCREMENTAL servers and keeps the first implementation safe.
    pub async fn did_change(&self, path: &Path, version: u64, text: &str) -> Result<(), LspError> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": path_to_uri(path)?,
                    "version": version,
                },
                "contentChanges": [{ "text": text }],
            }),
        )
        .await
    }

    pub async fn did_save(&self, path: &Path, text: Option<&str>) -> Result<(), LspError> {
        let mut params = json!({
            "textDocument": { "uri": path_to_uri(path)? }
        });
        if let Some(text) = text {
            params["text"] = Value::String(text.to_string());
        }
        self.notify("textDocument/didSave", params).await
    }

    pub async fn did_close(&self, path: &Path) -> Result<(), LspError> {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": path_to_uri(path)? } }),
        )
        .await
    }

    /// Gracefully shut the server down, then reap the process.
    pub async fn shutdown(&mut self) {
        self.state = ServerState::Stopping;
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            self.request("shutdown", Value::Null),
        )
        .await;
        let _ = self.notify("exit", Value::Null).await;
        if let Some(child) = &mut self.child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.state = ServerState::Stopped;
    }

    async fn send(&self, message: Value) -> Result<(), LspError> {
        self.outgoing
            .send(message)
            .await
            .map_err(|_| LspError::Closed)
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

async fn writer_task(stdin: ChildStdin, mut receiver: mpsc::Receiver<Value>) {
    let mut framed = FramedWrite::new(stdin, LspFrameCodec::new());
    while let Some(message) = receiver.recv().await {
        if framed.send(message).await.is_err() {
            break;
        }
    }
}

async fn reader_task(
    stdout: ChildStdout,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: mpsc::UnboundedSender<LspEvent>,
    server: String,
    outgoing: mpsc::Sender<Value>,
) {
    let mut framed = FramedRead::new(stdout, LspFrameCodec::new());
    while let Some(item) = framed.next().await {
        let Ok(message) = item else {
            break;
        };
        if !handle_message(&message, &pending, &events, &server, &outgoing).await {
            break;
        }
    }
    pending.lock().await.clear();
    let _ = events.send(LspEvent::Exited {
        server,
        status: None,
    });
}

async fn handle_message(
    message: &Value,
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: &mpsc::UnboundedSender<LspEvent>,
    server: &str,
    outgoing: &mpsc::Sender<Value>,
) -> bool {
    if let Some(id) = message.get("id").and_then(Value::as_i64) {
        if message.get("result").is_some() || message.get("error").is_some() {
            if let Some(sender) = pending.lock().await.remove(&id) {
                let _ = sender.send(message.clone());
            }
            return true;
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("unsupported client method: {method}"),
                }
            });
            let _ = outgoing.send(response).await;
            return true;
        }
    }

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return true;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "textDocument/publishDiagnostics" => {
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let entries = parse_diagnostics(&uri, params.get("diagnostics"));
            let _ = events.send(LspEvent::Diagnostics { uri, entries });
        }
        "window/logMessage" | "window/showMessage" => {
            let line = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let _ = events.send(LspEvent::Log {
                server: server.to_string(),
                line,
            });
        }
        _ => {}
    }
    true
}

async fn stderr_task(stderr: ChildStderr, events: mpsc::UnboundedSender<LspEvent>, server: String) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let _ = events.send(LspEvent::Log {
            server: server.clone(),
            line,
        });
    }
}

fn parse_diagnostics(uri: &str, diagnostics: Option<&Value>) -> Vec<DiagnosticEntry> {
    let Some(items) = diagnostics.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let line = item.pointer("/range/start/line")?.as_u64()? as u32;
            let character = item.pointer("/range/start/character")?.as_u64()? as u32;
            let message = item.get("message")?.as_str()?.to_string();
            let severity = item
                .get("severity")
                .and_then(Value::as_u64)
                .map(Severity::from_lsp)
                .unwrap_or(Severity::Error);
            let source = item
                .get("source")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(DiagnosticEntry {
                uri: uri.to_string(),
                line,
                character,
                severity,
                message,
                source,
            })
        })
        .collect()
}

fn parse_sync_kind(result: &Value) -> SyncKind {
    let sync = result.pointer("/capabilities/textDocumentSync");
    let change = match sync {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::Object(object)) => object.get("change").and_then(Value::as_u64),
        _ => None,
    };
    match change {
        Some(0) => SyncKind::None,
        Some(1) => SyncKind::Full,
        Some(2) => SyncKind::Incremental,
        _ => SyncKind::Incremental,
    }
}

pub fn path_to_uri(path: &Path) -> Result<String, LspError> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| LspError::InvalidPath(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_publish_diagnostics_payload() {
        let payload = json!([{
            "range": { "start": { "line": 4, "character": 2 }, "end": { "line": 4, "character": 5 } },
            "severity": 1,
            "message": "unused variable",
            "source": "rust-analyzer",
        }]);
        let entries = parse_diagnostics("file:///a.rs", Some(&payload));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].line, 4);
        assert_eq!(entries[0].severity, Severity::Error);
        assert_eq!(entries[0].source.as_deref(), Some("rust-analyzer"));
    }

    #[test]
    fn sync_kind_accepts_number_and_object_forms() {
        assert_eq!(
            parse_sync_kind(&json!({"capabilities": {"textDocumentSync": 1}})),
            SyncKind::Full
        );
        assert_eq!(
            parse_sync_kind(&json!({"capabilities": {"textDocumentSync": {"change": 2}}})),
            SyncKind::Incremental
        );
    }

    #[test]
    fn config_language_matching_supports_wildcards() {
        let config = LspServerConfig::new("x", "x", Vec::new(), vec!["rust".into()]);
        assert!(config.supports("rust"));
        assert!(!config.supports("python"));
        let wildcard = LspServerConfig::new("y", "y", Vec::new(), vec!["*".into()]);
        assert!(wildcard.supports("python"));
    }
}
