//! VS Code-style activity rail for the left sidebar.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Clear;

use crate::app::state::{AppState, SidebarView};

/// Width of the activity rail in columns.
pub const WIDTH: u16 = 4;

/// Split a sidebar area into `(activity rail, view content)`.
pub fn split(area: Rect) -> (Rect, Rect) {
    let width = WIDTH.min(area.width);
    (
        Rect::new(area.x, area.y, width, area.height),
        Rect::new(area.x + width, area.y, area.width - width, area.height),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    frame.render_widget(Clear, area);
    let buffer = frame.buffer_mut();
    buffer.set_style(area, Style::default().bg(theme.ui.status_bg));

    for (index, view) in SidebarView::ALL.iter().enumerate() {
        let y = area.y + index as u16 * 3;
        if y >= area.bottom() {
            break;
        }
        let height = 3.min(area.bottom() - y);
        let active = *view == state.sidebar_view;
        let item = Rect::new(area.x, y, area.width, height);
        if active {
            buffer.set_style(item, Style::default().bg(theme.ui.selection));
            put(
                buffer,
                area,
                area.x,
                y + 1,
                "▎",
                Style::default().fg(theme.ui.accent),
            );
        }
        put(
            buffer,
            area,
            area.x + 1,
            y + 1,
            view.glyph(),
            Style::default()
                .fg(if active {
                    theme.ui.accent
                } else {
                    theme.ui.dim
                })
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        );
    }
}

/// Resolve a click in the activity rail to a sidebar view.
pub fn hit_test(area: Rect, x: u16, y: u16) -> Option<SidebarView> {
    if !area.contains(Position::new(x, y)) {
        return None;
    }
    let index = (y - area.y) / 3;
    SidebarView::ALL.get(index as usize).copied()
}

fn put(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    x: u16,
    y: u16,
    symbol: &str,
    style: Style,
) {
    if !area.contains(Position::new(x, y)) {
        return;
    }
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_rail_width() {
        let (rail, content) = split(Rect::new(0, 0, 28, 40));
        assert_eq!(rail.width, WIDTH);
        assert_eq!(content.x, WIDTH);
        assert_eq!(content.width, 24);
    }

    #[test]
    fn hit_test_maps_rows_to_views() {
        let area = Rect::new(0, 0, 4, 40);
        assert_eq!(hit_test(area, 1, 1), Some(SidebarView::Explorer));
        assert_eq!(hit_test(area, 1, 4), Some(SidebarView::SourceControl));
        assert_eq!(hit_test(area, 1, 7), Some(SidebarView::Search));
        assert_eq!(hit_test(area, 1, 30), None);
    }
}
