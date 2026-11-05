//! File explorer panel.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::state::{AppState, FocusTarget};
use crate::git::GitFileStatus;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(theme.ui.panel_bg));
    let focused = state.focus == FocusTarget::Explorer;
    let border_style = Style::default().fg(if focused {
        theme.ui.border_focused
    } else {
        theme.ui.border
    });
    let block = Block::bordered()
        .border_style(border_style)
        .title(Line::from(vec![
            Span::styled(
                " Explorer ",
                Style::default()
                    .fg(if focused {
                        theme.ui.accent
                    } else {
                        theme.ui.dim
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{} ", truncate(&state.workspace_name(), 10)),
                Style::default().fg(theme.ui.dim),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame
        .buffer_mut()
        .set_style(inner, Style::default().bg(theme.ui.panel_bg));

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let content_x = inner.x + 1;
    let content_width = inner.width.saturating_sub(2) as usize;
    let footer_height = u16::from(inner.height >= 6);
    let list_height = inner.height.saturating_sub(footer_height) as usize;

    if footer_height > 0 {
        let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " a new · R rename ",
                Style::default().fg(theme.ui.dim),
            ))),
            footer,
        );
    }

    let entries = state.explorer.visible_entries();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    " No files yet",
                    Style::default().fg(theme.ui.foreground),
                )),
                Line::from(Span::styled(
                    " a creates a file",
                    Style::default().fg(theme.ui.dim),
                )),
            ]),
            Rect::new(content_x, inner.y, content_width as u16, list_height as u16),
        );
        return;
    }

    let selected = state.explorer.selected_index();
    let start = if entries.len() > list_height {
        selected.saturating_sub(list_height.saturating_sub(1))
    } else {
        0
    };

    let mut lines = Vec::new();
    for (index, entry) in entries.iter().enumerate().skip(start).take(list_height) {
        let is_selected = index == selected;
        let indent = entry.depth * 2;
        let icon = if entry.is_dir {
            if entry.expanded { "▾ " } else { "▸ " }
        } else {
            "· "
        };
        let badge = entry.git.map(|status| status.badge().to_string());
        let badge_width = badge.as_ref().map_or(0, |badge| badge.len() + 1);
        let name_width = content_width.saturating_sub(indent + 2 + badge_width);
        let name = truncate(&entry.name, name_width.max(1));

        let mut spans = vec![Span::raw(" ".repeat(indent))];
        spans.push(Span::styled(
            icon,
            Style::default().fg(if entry.is_dir {
                theme.ui.accent
            } else {
                theme.ui.dim
            }),
        ));
        spans.push(Span::styled(
            name.clone(),
            Style::default().fg(theme.ui.foreground),
        ));
        let used = indent + 2 + name.chars().count();
        let padding = content_width.saturating_sub(used + badge_width);
        if padding > 0 {
            spans.push(Span::raw(" ".repeat(padding)));
        }
        if let Some(status) = entry.git {
            spans.push(Span::styled(
                format!("{} ", status.badge()),
                Style::default().fg(git_color(status, state)),
            ));
        }

        let mut line = Line::from(spans);
        if is_selected {
            line = line.style(Style::default().bg(theme.ui.selection));
        }
        lines.push(line);
    }
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(content_x, inner.y, content_width as u16, list_height as u16),
    );
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut truncated: String = text.chars().take(width - 1).collect();
    truncated.push('…');
    truncated
}

fn git_color(status: GitFileStatus, state: &AppState) -> ratatui::style::Color {
    match status {
        GitFileStatus::Modified => state.theme.ui.warning,
        GitFileStatus::Added | GitFileStatus::Untracked => state.theme.ui.success,
        GitFileStatus::Deleted | GitFileStatus::Conflict => state.theme.ui.error,
        GitFileStatus::Renamed => state.theme.ui.hint,
    }
}
