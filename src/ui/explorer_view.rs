//! File explorer panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::state::{AppState, FocusTarget};
use crate::git::GitFileStatus;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    let focused = state.focus == FocusTarget::Explorer;
    let border_style = Style::default().fg(if focused {
        theme.ui.border_focused
    } else {
        theme.ui.border
    });
    let block = Block::bordered()
        .border_style(border_style)
        .title(Line::from(Span::styled(
            " Explorer ",
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

    let entries = state.explorer.visible_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " empty workspace",
                Style::default().fg(theme.ui.dim),
            ))),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = state.explorer.selected_index();
    let start = if entries.len() > height {
        selected.saturating_sub(height.saturating_sub(1))
    } else {
        0
    };

    let mut lines = Vec::new();
    for (index, entry) in entries.iter().enumerate().skip(start).take(height) {
        let is_selected = index == selected;
        let mut spans = Vec::new();
        spans.push(Span::raw(" ".repeat(entry.depth * 2)));
        if entry.is_dir {
            spans.push(Span::styled(
                if entry.expanded { "▾ " } else { "▸ " },
                Style::default().fg(theme.ui.accent),
            ));
            spans.push(Span::styled(
                entry.name.clone(),
                Style::default().fg(theme.ui.foreground),
            ));
        } else {
            spans.push(Span::styled("· ", Style::default().fg(theme.ui.dim)));
            spans.push(Span::styled(
                entry.name.clone(),
                Style::default().fg(theme.ui.foreground),
            ));
        }
        if let Some(status) = entry.git {
            spans.push(Span::styled(
                format!(" {}", status.badge()),
                Style::default().fg(git_color(status, state)),
            ));
        }
        let mut line = Line::from(spans);
        if is_selected {
            line = line.style(
                Style::default()
                    .bg(theme.ui.selection)
                    .add_modifier(Modifier::BOLD),
            );
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn git_color(status: GitFileStatus, state: &AppState) -> ratatui::style::Color {
    match status {
        GitFileStatus::Modified => state.theme.ui.warning,
        GitFileStatus::Added | GitFileStatus::Untracked => state.theme.ui.success,
        GitFileStatus::Deleted | GitFileStatus::Conflict => state.theme.ui.error,
        GitFileStatus::Renamed => state.theme.ui.hint,
    }
}
