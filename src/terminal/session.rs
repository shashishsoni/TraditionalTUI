//! One PTY session backed by a dedicated reader thread and `vt100` parser.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use vt100::Parser;

use super::TerminalError;

/// A live shell running inside a pseudo terminal.
pub struct PtySession {
    pub id: usize,
    pub title: String,
    parser: Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: Receiver<Vec<u8>>,
    exit_status: Option<String>,
    running: bool,
}

impl std::fmt::Debug for PtySession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PtySession")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("running", &self.running)
            .field("exit_status", &self.exit_status)
            .finish_non_exhaustive()
    }
}

impl PtySession {
    /// Spawn `shell` in `cwd` on a fresh PTY.
    pub fn spawn(
        id: usize,
        title: String,
        shell: &Path,
        cwd: &Path,
        rows: u16,
        cols: u16,
        scrollback: usize,
    ) -> Result<Self, TerminalError> {
        Self::spawn_program(id, title, shell, &[], cwd, rows, cols, scrollback)
    }

    /// Spawn an arbitrary program (used by ACP `terminal/create`).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_program(
        id: usize,
        title: String,
        program: &Path,
        args: &[String],
        cwd: &Path,
        rows: u16,
        cols: u16,
        scrollback: usize,
    ) -> Result<Self, TerminalError> {
        let pty_system = native_pty_system();
        let pair: PtyPair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TerminalError::Pty(error.to_string()))?;

        let mut command = CommandBuilder::new(program.as_os_str());
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TTUI", "1");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TerminalError::Pty(error.to_string()))?;

        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name(format!("ttui-pty-{id}"))
            .spawn(move || read_loop(reader, tx))
            .map_err(TerminalError::Io)?;

        Ok(Self {
            id,
            title,
            parser: Parser::new(rows, cols, scrollback),
            master: pair.master,
            writer,
            child,
            rx,
            exit_status: None,
            running: true,
        })
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), TerminalError> {
        let size = PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .resize(size)
            .map_err(|error| TerminalError::Pty(error.to_string()))?;
        self.parser.set_size(size.rows, size.cols);
        Ok(())
    }

    /// Process queued output. Returns the number of bytes consumed.
    pub fn drain(&mut self) -> usize {
        let mut total = 0;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => {
                    total += bytes.len();
                    self.parser.process(&bytes);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    break;
                }
            }
        }
        if self.running {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.running = false;
                self.exit_status = Some(format!("{status:?}"));
            }
        }
        total
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn exit_status(&self) -> Option<&str> {
        self.exit_status.as_deref()
    }

    pub fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_loop(mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if tx.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}
