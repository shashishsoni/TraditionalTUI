//! Newline-delimited JSON-RPC 2.0 transport for ACP agents.
//!
//! ACP frames are one JSON object per line over the child's `stdin`/`stdout`.
//! The reader task resolves pending client requests and surfaces agent
//! notifications and agent-to-client requests as [`AcpEvent`] values.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, mpsc, oneshot};

use super::AcpError;
use crate::platform::process::configure_tokio_command;

/// Events surfaced to the IDE.
#[derive(Debug, Clone)]
pub enum AcpEvent {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Stderr(String),
    Exited(Option<String>),
}

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, AcpError>>>>>;

/// A live agent subprocess and its JSON-RPC connection.
pub struct AcpConnection {
    child: std::sync::Mutex<Option<Child>>,
    outgoing: mpsc::Sender<Value>,
    pending: Pending,
    next_id: AtomicI64,
    events_rx: std::sync::Mutex<mpsc::UnboundedReceiver<AcpEvent>>,
}

impl std::fmt::Debug for AcpConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpConnection")
            .finish_non_exhaustive()
    }
}

impl AcpConnection {
    /// Spawn an agent with piped stdio.
    pub async fn spawn(command: &str, args: &[String], cwd: &Path) -> Result<Self, AcpError> {
        let mut process = tokio::process::Command::new(command);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("GEMINI_API_KEY");
        configure_tokio_command(&mut process);

        let mut child = process.spawn().map_err(|source| AcpError::Spawn {
            agent: command.to_string(),
            source,
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpError::Protocol("agent stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpError::Protocol("agent stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AcpError::Protocol("agent stderr unavailable".into()))?;

        let (outgoing, outgoing_rx) = mpsc::channel(256);
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(writer_task(stdin, outgoing_rx));
        tokio::spawn(reader_task(stdout, pending.clone(), events_tx.clone()));
        tokio::spawn(stderr_task(stderr, events_tx));

        Ok(Self {
            child: std::sync::Mutex::new(Some(child)),
            outgoing,
            pending,
            next_id: AtomicI64::new(1),
            events_rx: std::sync::Mutex::new(events_rx),
        })
    }

    /// Perform the ACP `initialize` handshake.
    pub async fn initialize(&self) -> Result<Value, AcpError> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientInfo": {
                    "name": "TradinationalTUI",
                    "version": crate::VERSION,
                },
                "clientCapabilities": {
                    "fs": {
                        "readTextFile": true,
                        "writeTextFile": true,
                    },
                    "terminal": true,
                },
            }),
        )
        .await
    }

    /// Create a session rooted at `cwd` and return its id.
    pub async fn new_session(&self, cwd: &Path) -> Result<String, AcpError> {
        let result = self
            .request(
                "session/new",
                json!({
                    "cwd": cwd.to_string_lossy(),
                    "mcpServers": [],
                }),
            )
            .await?;
        result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AcpError::Protocol("session/new did not return sessionId".into()))
    }

    /// Send a prompt turn and await the agent's completion result.
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<Value, AcpError> {
        self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )
        .await
    }

    pub async fn cancel(&self, session_id: &str) -> Result<(), AcpError> {
        self.notify("session/cancel", json!({ "sessionId": session_id }))
            .await
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, AcpError> {
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

        match tokio::time::timeout(Duration::from_secs(30), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AcpError::NotRunning),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(AcpError::Timeout)
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AcpError> {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), AcpError> {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
        .await
    }

    pub async fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<(), AcpError> {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }))
        .await
    }

    /// Drain pending agent events.
    pub fn poll_events(&self) -> Vec<AcpEvent> {
        let mut events = Vec::new();
        if let Ok(mut receiver) = self.events_rx.lock() {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }
        events
    }

    pub fn has_pending_events(&self) -> bool {
        self.events_rx
            .lock()
            .map(|receiver| !receiver.is_empty())
            .unwrap_or(false)
    }

    pub async fn shutdown(&self) {
        let child = self.child.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.pending.lock().await.clear();
    }

    async fn send(&self, message: Value) -> Result<(), AcpError> {
        self.outgoing
            .send(message)
            .await
            .map_err(|_| AcpError::NotRunning)
    }
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        let child = self.child.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            let _ = child.start_kill();
        }
    }
}

async fn writer_task(mut stdin: ChildStdin, mut receiver: mpsc::Receiver<Value>) {
    while let Some(message) = receiver.recv().await {
        let Ok(mut line) = serde_json::to_vec(&message) else {
            continue;
        };
        line.push(b'\n');
        if stdin.write_all(&line).await.is_err() {
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }
}

async fn reader_task(
    stdout: ChildStdout,
    pending: Pending,
    events: mpsc::UnboundedSender<AcpEvent>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(message) => dispatch(message, &pending, &events).await,
            Err(error) => {
                let _ = events.send(AcpEvent::Stderr(format!("invalid ACP JSON: {error}")));
            }
        }
    }
    pending.lock().await.clear();
    let _ = events.send(AcpEvent::Exited(None));
}

async fn stderr_task(stderr: ChildStderr, events: mpsc::UnboundedSender<AcpEvent>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let _ = events.send(AcpEvent::Stderr(line));
    }
}

async fn dispatch(message: Value, pending: &Pending, events: &mpsc::UnboundedSender<AcpEvent>) {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);

    let is_response = message.get("result").is_some() || message.get("error").is_some();
    if let Some(number) = id.as_ref().and_then(Value::as_i64) {
        if is_response {
            if let Some(sender) = pending.lock().await.remove(&number) {
                let outcome = match message.get("error") {
                    Some(error) => Err(AcpError::Protocol(error.to_string())),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = sender.send(outcome);
            }
            return;
        }
    }

    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match (id, method) {
        (Some(id), Some(method)) => {
            let _ = events.send(AcpEvent::Request { id, method, params });
        }
        (None, Some(method)) => {
            let _ = events.send(AcpEvent::Notification { method, params });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_resolves_pending_requests() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();
        pending.lock().await.insert(1, sender);
        let (events, _events_rx) = mpsc::unbounded_channel();

        dispatch(json!({"id": 1, "result": {"ok": true}}), &pending, &events).await;
        let result = receiver.await.expect("resolved").expect("ok");
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn dispatch_surfaces_notifications_and_requests() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (events, mut events_rx) = mpsc::unbounded_channel();

        dispatch(
            json!({"method": "session/update", "params": {"kind": "chunk"}}),
            &pending,
            &events,
        )
        .await;
        match events_rx.recv().await.expect("event") {
            AcpEvent::Notification { method, .. } => assert_eq!(method, "session/update"),
            other => panic!("unexpected event: {other:?}"),
        }

        dispatch(
            json!({"id": 9, "method": "session/request_permission", "params": {}}),
            &pending,
            &events,
        )
        .await;
        match events_rx.recv().await.expect("event") {
            AcpEvent::Request { id, method, .. } => {
                assert_eq!(id, json!(9));
                assert_eq!(method, "session/request_permission");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_propagates_json_rpc_errors() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();
        pending.lock().await.insert(3, sender);
        let (events, _events_rx) = mpsc::unbounded_channel();

        dispatch(
            json!({"id": 3, "error": {"code": -32602, "message": "bad params"}}),
            &pending,
            &events,
        )
        .await;
        assert!(matches!(
            receiver.await.expect("resolved"),
            Err(AcpError::Protocol(_))
        ));
    }
}
