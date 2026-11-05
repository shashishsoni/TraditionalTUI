//! Workbench compositor.

use ratatui::Frame;
use ratatui::style::Style;

use crate::app::state::{AppState, SidebarView};
use crate::ui::layout::compute;
use crate::ui::{
    activity_bar, ai_panel, bottom_panel, editor_view, explorer_view, overlay, search_view,
    source_control_view, status_bar,
};

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
            let (activity, sidebar) = activity_bar::split(explorer);
            activity_bar::render(frame, activity, state);
            match state.sidebar_view {
                SidebarView::Explorer => explorer_view::render(frame, sidebar, state),
                SidebarView::SourceControl => source_control_view::render(frame, sidebar, state),
                SidebarView::Search => search_view::render(frame, sidebar, state),
            }
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
    use ratatui::layout::Rect;

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

    #[test]
    fn horizontal_scroll_renders_the_line_tail() {
        let content = render_content(120, 40, |state| {
            let document = state.active_doc_mut().expect("document");
            document.rope = ropey::Rope::from_str(&format!("{}TAIL", "a".repeat(150)));
            document.left_col = 150;
        });
        assert!(content.contains("TAIL"));
    }

    #[test]
    fn long_lines_do_not_bleed_into_the_side_panel() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        state.viewport = (140, 40);
        state.active_doc_mut().expect("document").rope = ropey::Rope::from_str(&"X".repeat(500));
        let backend = TestBackend::new(140, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| UiRenderer.render(frame, &state))
            .expect("draw");
        let ai = crate::ui::layout::compute(Rect::new(0, 0, 140, 40), &state.layout)
            .ai
            .expect("ai panel");
        let buffer = terminal.backend().buffer();
        for y in ai.y..ai.bottom() {
            for x in ai.x..ai.right() {
                if let Some(cell) = buffer.cell((x, y)) {
                    assert_ne!(cell.symbol(), "X", "editor bled into AI panel at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn activity_bar_views_render() {
        let content = render_content(150, 44, |state| {
            state.set_sidebar_view(SidebarView::SourceControl);
        });
        assert!(content.contains("Source Control"));
        assert!(content.contains("No Git repository"));

        let content = render_content(150, 44, |state| {
            state.set_sidebar_view(SidebarView::Search);
        });
        assert!(content.contains("Search"));
        assert!(content.contains("type a query"));
    }

    #[test]
    fn ai_panel_renders_inline_red_green_diff() {
        let content = render_content(160, 50, |state| {
            state.diff_preview = Some(crate::ai::DiffPreview::compute(
                std::path::PathBuf::from("src/main.rs"),
                "fn main() {\n    old();\n}\n",
                "fn main() {\n    new();\n}\n",
            ));
        });
        assert!(content.contains("Edit preview"));
        assert!(content.contains("old();"));
        assert!(content.contains("new();"));
    }

    #[test]
    fn diff_overlay_renders() {
        let content = render_content(160, 50, |state| {
            state.diff_preview = Some(crate::ai::DiffPreview::compute(
                std::path::PathBuf::from("src/main.rs"),
                "old();\n",
                "new();\n",
            ));
            state.active_modal = Some(ModalState::new(ModalKind::DiffPreview));
        });
        assert!(content.contains("AI Edit Preview"));
        assert!(content.contains("old();"));
        assert!(content.contains("new();"));
    }

    #[test]
    fn inline_diff_uses_red_and_green_styles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_testing(dir.path());
        state.viewport = (160, 50);
        state.diff_preview = Some(crate::ai::DiffPreview::compute(
            std::path::PathBuf::from("src/main.rs"),
            "old();\n",
            "new();\n",
        ));
        let backend = TestBackend::new(160, 50);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| UiRenderer.render(frame, &state))
            .expect("draw");
        let ai = crate::ui::layout::compute(Rect::new(0, 0, 160, 50), &state.layout)
            .ai
            .expect("ai panel");
        let buffer = terminal.backend().buffer();
        let mut removed_red = false;
        let mut added_green = false;
        for y in ai.y..ai.bottom() {
            for x in ai.x..ai.right() {
                if let Some(cell) = buffer.cell((x, y)) {
                    if cell.symbol() == "-" {
                        removed_red |= cell.style().fg == Some(state.theme.ui.error);
                    }
                    if cell.symbol() == "+" {
                        added_green |= cell.style().fg == Some(state.theme.ui.success);
                    }
                }
            }
        }
        assert!(removed_red, "removed lines must be red");
        assert!(added_green, "added lines must be green");
    }
}
