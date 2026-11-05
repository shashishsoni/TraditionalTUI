//! Search sidebar view with project-wide results.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::state::{AppState, FocusTarget};
use crate::platform::paths::relative_display;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    let focused = state.focus == FocusTarget::Explorer;
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(theme.ui.panel_bg));
    let block = Block::bordered()
        .border_style(Style::default().fg(if focused {
            theme.ui.border_focused
        } else {
            theme.ui.border
        }))
        .title(Line::from(Span::styled(
            " Search ",
            Style::default()
                .fg(if focused {
                    theme.ui.accent
                } else {
                    theme.ui.dim
                })
                .add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame
        .buffer_mut()
        .set_style(inner, Style::default().bg(theme.ui.panel_bg));
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let content = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let search = &state.sidebar_search;

    let line = Line::from(vec![
        Span::styled(
            "⌕ ",
            Style::default()
                .fg(theme.ui.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            search.query.clone(),
            Style::default().fg(theme.ui.foreground),
        ),
        Span::styled("_", Style::default().fg(theme.ui.cursor)),
    ]);
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(content.x, inner.y, content.width, 1),
    );

    let case_badge = if search.case_sensitive {
        "[Aa]"
    } else {
        "[aa]"
    };
    let word_badge = if search.whole_word { "[=]" } else { "[~]" };
    let status = if search.searching || search.busy {
        format!(" searching… {case_badge} case {word_badge} word")
    } else if search.query.trim().is_empty() {
        " type a query and press Enter".to_string()
    } else {
        format!(
            " {} result(s)  {case_badge} case  {word_badge} word  Alt+C/W toggles",
            search.results.len()
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            status,
            Style::default().fg(theme.ui.dim),
        ))),
        Rect::new(content.x, inner.y + 1, content.width, 1),
    );

    let footer_height = u16::from(inner.height >= 6);
    if footer_height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Enter search · Esc ",
                Style::default().fg(theme.ui.dim),
            ))),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }

    let list_height = inner.height.saturating_sub(2).saturating_sub(footer_height) as usize;
    if list_height == 0 {
        return;
    }

    if !search.busy && !search.query.trim().is_empty() && search.results.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No matches",
                Style::default().fg(theme.ui.dim),
            ))),
            Rect::new(content.x, inner.y + 2, content.width, list_height as u16),
        );
        return;
    }

    let selected = search.selected.min(search.results.len().saturating_sub(1));
    let start = selected.saturating_sub(list_height.saturating_sub(1));
    let mut lines = Vec::new();
    for (index, result) in search
        .results
        .iter()
        .enumerate()
        .skip(start)
        .take(list_height)
    {
        let path = relative_display(state.workspace.root(), &result.path);
        let mut line_spans = vec![
            Span::styled(path, Style::default().fg(theme.ui.hint)),
            Span::styled(
                format!(":{}  ", result.line),
                Style::default().fg(theme.ui.dim),
            ),
            Span::styled(
                result.text.trim().to_string(),
                Style::default().fg(theme.ui.foreground),
            ),
        ];
        if index == selected {
            line_spans.push(Span::styled(" ◀", Style::default().fg(theme.ui.accent)));
        }
        let mut line = Line::from(line_spans);
        if index == selected {
            line = line.style(Style::default().bg(theme.ui.selection));
        }
        lines.push(line);
    }
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(content.x, inner.y + 2, content.width, list_height as u16),
    );
}

/// Result index for a click inside the results list, accounting for scrolling.
pub fn result_index_at(area: Rect, row: u16, selected: usize, total: usize) -> Option<usize> {
    let inner = Block::bordered().inner(area);
    let start = inner.y.saturating_add(2);
    let footer = u16::from(inner.height >= 6);
    if row < start || row >= inner.bottom().saturating_sub(footer) {
        return None;
    }
    let height = inner.height.saturating_sub(2).saturating_sub(footer) as usize;
    let list_start = if total > height {
        selected.saturating_sub(height.saturating_sub(1))
    } else {
        0
    };
    let index = list_start + (row - start) as usize;
    (index < total).then_some(index)
}

/// Whether `row` is the query input row.
pub fn is_input_row(area: Rect, row: u16) -> bool {
    Block::bordered().inner(area).y == row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_hit_test_accounts_for_scrolling() {
        let area = Rect::new(10, 5, 80, 20);
        // inner.y = 6, list starts at 8.
        assert_eq!(result_index_at(area, 8, 0, 3), Some(0));
        assert_eq!(result_index_at(area, 9, 1, 3), Some(1));
        assert_eq!(result_index_at(area, 6, 0, 3), None);
        assert_eq!(result_index_at(area, 40, 0, 3), None);
    }
}
