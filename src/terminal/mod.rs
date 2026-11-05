//! Integrated PTY terminal multiplexer.
//!
//! `portable_pty` provides Unix PTYs and Windows ConPTY; `vt100` maintains the
//! screen grid. Raw bytes never reach the host terminal: the UI renders the
//! parsed screen cells into the bottom panel.

mod query;
mod session;

pub use session::PtySession;

use std::path::Path;
use thiserror::Error;

use crate::platform::shell::shell_display_name;

/// Terminal failure.
#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PTY error: {0}")]
    Pty(String),

    #[error("terminal session {0} does not exist")]
    NotFound(usize),
}

/// Owns all PTY sessions shown in the bottom panel.
#[derive(Debug, Default)]
pub struct TerminalManager {
    sessions: Vec<PtySession>,
    active: usize,
    next_id: usize,
    scrollback: usize,
}

impl TerminalManager {
    /// Create an empty manager with the configured scrollback depth.
    pub fn new(scrollback: usize) -> Self {
        Self {
            sessions: Vec::new(),
            active: 0,
            next_id: 1,
            scrollback,
        }
    }

    pub fn set_scrollback(&mut self, scrollback: usize) {
        self.scrollback = scrollback.max(100);
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn active_index(&self) -> Option<usize> {
        if self.sessions.is_empty() {
            None
        } else {
            Some(self.active.min(self.sessions.len() - 1))
        }
    }

    pub fn active(&self) -> Option<&PtySession> {
        self.active_index().map(|index| &self.sessions[index])
    }

    pub fn active_mut(&mut self) -> Option<&mut PtySession> {
        let index = self.active_index()?;
        self.sessions.get_mut(index)
    }

    pub fn sessions(&self) -> &[PtySession] {
        &self.sessions
    }

    /// Whether at least one shell is still running.
    pub fn has_live_sessions(&self) -> bool {
        self.sessions.iter().any(PtySession::is_running)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.sessions.len() {
            self.active = index;
        }
    }

    /// Spawn a shell session and make it active.
    pub fn spawn(
        &mut self,
        shell: &Path,
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<usize, TerminalError> {
        self.spawn_program(shell, &[], cwd, rows, cols, None)
    }

    /// Spawn an arbitrary program in a new PTY.
    pub fn spawn_program(
        &mut self,
        program: &Path,
        args: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        title: Option<String>,
    ) -> Result<usize, TerminalError> {
        let id = self.next_id;
        self.next_id += 1;
        let title = title.unwrap_or_else(|| format!("{} {}", shell_display_name(program), id));
        let session = PtySession::spawn_program(
            id,
            title,
            program,
            args,
            cwd,
            rows.max(2),
            cols.max(2),
            self.scrollback,
        )?;
        self.sessions.push(session);
        self.active = self.sessions.len() - 1;
        Ok(id)
    }

    pub fn session_by_id(&self, id: usize) -> Option<&PtySession> {
        self.sessions.iter().find(|session| session.id == id)
    }

    pub fn close_by_id(&mut self, id: usize) {
        if let Some(index) = self.sessions.iter().position(|session| session.id == id) {
            self.sessions.remove(index);
            if self.active >= self.sessions.len() && !self.sessions.is_empty() {
                self.active = self.sessions.len() - 1;
            }
        }
    }

    pub fn screen_contents(&self, id: usize) -> Option<String> {
        self.session_by_id(id)
            .map(|session| session.screen().contents())
    }

    pub fn write_active(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        self.active_mut()
            .ok_or(TerminalError::NotFound(0))?
            .write(bytes)
    }

    pub fn resize_active(&mut self, rows: u16, cols: u16) {
        if let Some(session) = self.active_mut() {
            let _ = session.resize(rows, cols);
        }
    }

    /// Feed pending PTY output into every parser. Returns bytes processed.
    pub fn drain(&mut self) -> usize {
        self.sessions
            .iter_mut()
            .map(|session| session.drain())
            .sum()
    }

    pub fn close_active(&mut self) {
        if let Some(index) = self.active_index() {
            self.sessions.remove(index);
            if self.active >= self.sessions.len() && !self.sessions.is_empty() {
                self.active = self.sessions.len() - 1;
            }
        }
    }

    pub fn close_all(&mut self) {
        self.sessions.clear();
        self.active = 0;
    }
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn pty_captures_command_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manager = TerminalManager::new(200);
        let id = manager
            .spawn_program(
                Path::new("/bin/sh"),
                &["-c".to_string(), "printf 'pty-ok'".to_string()],
                dir.path(),
                24,
                80,
                Some("test".to_string()),
            )
            .expect("spawn");

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            manager.drain();
            if manager
                .screen_contents(id)
                .is_some_and(|contents| contents.contains("pty-ok"))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("PTY never produced expected output");
    }

    #[test]
    fn closing_session_by_id_removes_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut manager = TerminalManager::new(200);
        let id = manager
            .spawn_program(
                Path::new("/bin/sh"),
                &["-c".to_string(), "sleep 5".to_string()],
                dir.path(),
                10,
                40,
                None,
            )
            .expect("spawn");
        assert!(manager.session_by_id(id).is_some());
        manager.close_by_id(id);
        assert!(manager.session_by_id(id).is_none());
    }
}
