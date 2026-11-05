//! Application runtime: single-owner state, event loop and subsystem wiring.

mod action;
mod editor_input;
mod event;
mod events;
mod input;
mod modal_input;
pub mod state;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyEventKind};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::ai::AiEvent;
use crate::ui::{TuiTerminal, UiRenderer};

use self::event::AppEvent;
use self::state::{AppState, BottomTab, FocusTarget};

/// Which LSP synchronization message to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LspSync {
    Open,
    Save,
    Change,
}

struct LspRequest {
    path: PathBuf,
    language: String,
    sync: LspSync,
}

/// Top-level application.
pub struct App {
    pub state: AppState,
    ui: UiRenderer,
    event_tx: mpsc::Sender<AppEvent>,
    event_rx: mpsc::Receiver<AppEvent>,
    ai_events: Option<mpsc::Receiver<AiEvent>>,
    ai_cancel: Option<CancellationToken>,
    lsp_request: Option<LspRequest>,
    lsp_dirty: bool,
    lsp_dirty_at: Option<Instant>,
    lsp_busy: bool,
    lsp_generation: u64,
    acp_generation: u64,
    dirty: bool,
}

impl App {
    pub fn new(state: AppState) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1_024);
        let mut app = Self {
            state,
            ui: UiRenderer::new(),
            event_tx,
            event_rx,
            ai_events: None,
            ai_cancel: None,
            lsp_request: None,
            lsp_dirty: false,
            lsp_dirty_at: None,
            lsp_busy: false,
            lsp_generation: 0,
            acp_generation: 0,
            dirty: true,
        };
        app.queue_lsp_current(LspSync::Open);
        app
    }

    /// Run until the user quits.
    pub async fn run(mut self, terminal: &mut TuiTerminal) -> crate::Result<()> {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("ttui-input".into())
            .spawn(move || {
                while let Ok(event) = crossterm::event::read() {
                    if input_tx.send(event).is_err() {
                        break;
                    }
                }
            })?;

        let mut tick = tokio::time::interval(Duration::from_millis(33));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        if let Ok(size) = terminal.size() {
            self.state.viewport = (size.width.max(1), size.height.max(1));
            let (rows, cols) = self.state.terminal_size();
            self.state.terminal.resize_active(rows, cols);
        }
        self.start_background_tasks();

        while !self.state.should_quit {
            if self.dirty {
                terminal.draw(|frame| self.ui.render(frame, &self.state))?;
                self.dirty = false;
            }

            let pending_tick = self.has_pending_work();
            tokio::select! {
                biased;

                Some(event) = input_rx.recv() => {
                    self.handle_terminal_event(event).await;
                    while let Ok(next) = input_rx.try_recv() {
                        self.handle_terminal_event(next).await;
                    }
                    self.dirty = true;
                }

                Some(event) = self.event_rx.recv() => {
                    self.handle_app_event(event).await;
                    self.dirty = true;
                }

                _ = tick.tick(), if pending_tick => {
                    if self.tick().await {
                        self.dirty = true;
                    }
                }
            }
        }

        self.shutdown().await;
        Ok(())
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    async fn handle_terminal_event(&mut self, event: Event) {
        tracing::debug!(event = ?event, "raw terminal event");
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return;
                }
                self.handle_key(key).await;
                self.mark_dirty();
            }
            Event::Resize(width, height) => {
                self.state.viewport = (width.max(1), height.max(1));
                let (rows, cols) = self.state.terminal_size();
                self.state.terminal.resize_active(rows, cols);
                self.mark_dirty();
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
                self.mark_dirty();
            }
            Event::Paste(text) => {
                self.handle_paste(&text);
                self.mark_dirty();
            }
            Event::FocusGained | Event::FocusLost => {}
        }
    }

    fn handle_paste(&mut self, text: &str) {
        if let Some(modal) = self.state.modal_state.as_mut() {
            if modal.kind.has_input() {
                modal.query.push_str(text);
                let mut modal = modal.clone();
                self.rebuild_modal_entries(&mut modal);
                if let Some(active) = self.state.modal_state.as_mut() {
                    active.entries = modal.entries;
                    active.selected = modal.selected;
                }
                return;
            }
        }
        if matches!(self.state.focus, FocusTarget::Bottom(BottomTab::Terminal))
            && self.state.terminal.active().is_some()
        {
            let _ = self.state.terminal.write_active(text.as_bytes());
            return;
        }
        if let Some(document) = self.state.active_doc_mut() {
            if document
                .insert_text(text, crate::editor::EditKind::Paste)
                .is_ok()
            {
                self.note_edit();
            }
        }
    }

    async fn shutdown(&mut self) {
        if let Some(token) = self.ai_cancel.take() {
            token.cancel();
        }
        self.state.terminal.close_all();
        if let Err(error) = self.state.workspace.save_session() {
            tracing::warn!(%error, "failed to save workspace session");
        }
        self.state.acp.shutdown().await;
        self.state.lsp.shutdown_all().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn app_constructs_without_network_or_agents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::for_testing(dir.path());
        let app = App::new(state);
        assert!(!app.state.should_quit);
        assert!(!app.state.ai.is_configured());
        assert!(app.state.acp.agent_name().is_none());
    }
}
