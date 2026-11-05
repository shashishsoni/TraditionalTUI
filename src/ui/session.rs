//! Panic-safe terminal lifecycle and synchronized output backend.

use std::io::{self, Stdout, Write};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType as CrosstermClearType, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

/// Ratatui terminal type used by the app.
pub type TuiTerminal = Terminal<SyncBackend<CrosstermBackend<Stdout>>>;

const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";

/// Backend wrapper that frames every flush with DEC Mode 2026.
#[derive(Debug)]
pub struct SyncBackend<B: Backend> {
    inner: B,
}

impl<B: Backend> SyncBackend<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

impl<B: Backend> Backend for SyncBackend<B> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.write_all(SYNC_BEGIN)?;
        stdout.flush()?;
        self.inner.flush()?;
        stdout.write_all(SYNC_END)?;
        stdout.flush()
    }
}

/// RAII terminal guard: raw mode, alternate screen, mouse capture and paste.
pub struct TerminalSession {
    terminal: TuiTerminal,
    active: bool,
}

impl TerminalSession {
    /// Enter the interactive terminal screen.
    pub fn enter(mouse: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            Clear(CrosstermClearType::All)
        ) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        if mouse {
            let _ = execute!(stdout, EnableMouseCapture);
        }
        let backend = SyncBackend::new(CrosstermBackend::new(io::stdout()));
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                restore_terminal();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            active: true,
        })
    }

    pub fn terminal_mut(&mut self) -> &mut TuiTerminal {
        &mut self.terminal
    }

    pub fn restore(&mut self) {
        if self.active {
            self.active = false;
            restore_terminal();
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Install a panic hook that restores the terminal before printing the panic.
pub fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        let _ = writeln!(io::stderr(), "ttui panicked: {info}");
        original(info);
    }));
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = stdout.write_all(SYNC_END);
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_sequences_are_well_formed() {
        assert_eq!(SYNC_BEGIN, b"\x1b[?2026h");
        assert_eq!(SYNC_END, b"\x1b[?2026l");
    }
}
