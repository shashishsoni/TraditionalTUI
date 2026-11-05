//! Workbench compositor.

use ratatui::Frame;
use ratatui::style::Style;

use crate::app::state::AppState;
use crate::ui::layout::compute;
use crate::ui::{ai_panel, bottom_panel, editor_view, explorer_view, overlay, status_bar};

/// Renders the complete workbench into a frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct UiRenderer;

impl UiRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Pure presentation: reads immutable state, writes one frame.
    pub fn render(&self, frame: &mut Frame, state: &AppState) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let regions = compute(area, &state.layout);
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(state.theme.ui.background));

        if let Some(explorer) = regions.explorer {
            explorer_view::render(frame, explorer, state);
        }
        editor_view::render_tabs(frame, regions.tab_bar, state);
        editor_view::render(frame, regions.editor, state);
        if let Some(bottom) = regions.bottom {
            bottom_panel::render(frame, bottom, state);
        }
        if let Some(ai) = regions.ai {
            ai_panel::render(frame, ai, state);
        }
        status_bar::render(frame, regions.status, state);
        overlay::render(frame, area, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::{AppState, ModalAction, ModalEntry, ModalKind, ModalState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_content(width: u16, height: u16, mutate: impl FnOnce(&mut AppState)) -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        state.viewport = (width, height);
        mutate(&mut state);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| UiRenderer.render(frame, &state))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let mut content = String::new();
        for y in 0..height {
            for x in 0..width {
                if let Some(cell) = buffer.cell((x, y)) {
                    content.push_str(cell.symbol());
                }
            }
            content.push('\n');
        }
        content
    }

    #[test]
    fn renders_core_workbench_zones() {
        let content = render_content(160, 50, |_| {});
        assert!(content.contains("Explorer"));
        assert!(content.contains("fn main"));
        assert!(content.contains("AI Assistant"));
        assert!(content.contains("Terminal"));
        assert!(content.contains("INSERT"));
    }

    #[test]
    fn narrow_terminal_collapses_sidebars_without_panicking() {
        let content = render_content(70, 24, |_| {});
        assert!(content.contains("INSERT"));
        assert!(!content.contains("AI Assistant"));
    }

    #[test]
    fn command_palette_overlay_renders_entries() {
        let content = render_content(140, 44, |state| {
            let mut modal = ModalState::new(ModalKind::CommandPalette);
            modal.entries.push(ModalEntry::new(
                "Save File",
                "ctrl-s",
                ModalAction::Command("file.save".into()),
            ));
            state.active_modal = Some(modal);
        });
        assert!(content.contains("Command Palette"));
        assert!(content.contains("Save File"));
    }

    #[test]
    fn command_mode_shows_the_command_buffer() {
        let content = render_content(120, 40, |state| {
            state.mode = crate::app::state::EditMode::Command;
            state.command_buffer = "wq".into();
        });
        assert!(content.contains(":wq"));
    }

    #[test]
    fn configuration_document_renders() {
        let content = render_content(160, 50, |state| {
            let root = state.workspace.root().to_path_buf();
            let path = root.join("config.toml");
            std::fs::write(&path, crate::config::CONFIG_TEMPLATE).expect("write config");
            state.open_file(&path).expect("open config");
        });
        assert!(content.contains("ttui configuration"));
        assert!(content.contains("[editor]"));
    }
}
