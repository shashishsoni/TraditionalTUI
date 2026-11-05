//! Source Control sidebar view.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::state::{AppState, FocusTarget};
use crate::git::GitFileStatus;
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
            " Source Control ",
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
    let footer_height = u16::from(inner.height >= 6);
    if footer_height > 0 {
        let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " s stage · u unstage · b branch · r refresh ",
                Style::default().fg(theme.ui.dim),
            ))),
            footer,
        );
    }

    let badges = state.source_control_badges();
    let branch = state.git.as_ref().map(|git| git.branch_short());
    let branch_color = if state.git.is_some() {
        theme.ui.hint
    } else {
        theme.ui.dim
    };
    let staged_count = badges
        .values()
        .filter(|badge| badge.starts_with('S'))
        .count();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("⑂ ", Style::default().fg(branch_color)),
            Span::styled(
                branch.unwrap_or_else(|| "no repo".to_string()),
                Style::default()
                    .fg(theme.ui.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {staged_count} staged"),
                Style::default().fg(theme.ui.dim),
            ),
        ])),
        Rect::new(content.x, inner.y, content.width, 1),
    );

    if state.git.is_none() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No Git repository",
                    Style::default().fg(theme.ui.foreground),
                )),
                Line::from(Span::styled(
                    "Alt+O opens another folder",
                    Style::default().fg(theme.ui.dim),
                )),
            ]),
            Rect::new(
                content.x,
                inner.y + 2,
                content.width,
                inner.height.saturating_sub(2),
            ),
        );
        return;
    }

    let files = state.source_control_files();
    if files.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Working tree clean",
                Style::default().fg(theme.ui.success),
            ))),
            Rect::new(
                content.x,
                inner.y + 2,
                content.width,
                inner.height.saturating_sub(2),
            ),
        );
        return;
    }

    let list_height = inner.height.saturating_sub(2).saturating_sub(footer_height) as usize;
    let selected = state
        .source_control_selected
        .min(files.len().saturating_sub(1));
    let start = selected.saturating_sub(list_height.saturating_sub(1));
    let mut lines = Vec::new();
    for (index, (path, status, badge)) in files.iter().enumerate().skip(start).take(list_height) {
        let display = relative_display(state.workspace.root(), path);
        let mut line = Line::from(vec![
            Span::styled(
                format!("{badge:>5} "),
                Style::default()
                    .fg(git_color(*status, state))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(display, Style::default().fg(theme.ui.foreground)),
        ]);
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

/// Row index of the changed-file list for a click, accounting for scrolling.
pub fn file_index_at(area: Rect, row: u16, selected: usize, total: usize) -> Option<usize> {
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

/// Whether `row` is the branch header.
pub fn is_branch_row(area: Rect, row: u16) -> bool {
    let inner = Block::bordered().inner(area);
    row == inner.y
}

fn git_color(status: GitFileStatus, state: &AppState) -> ratatui::style::Color {
    match status {
        GitFileStatus::Modified => state.theme.ui.warning,
        GitFileStatus::Added | GitFileStatus::Untracked => state.theme.ui.success,
        GitFileStatus::Deleted | GitFileStatus::Conflict => state.theme.ui.error,
        GitFileStatus::Renamed => state.theme.ui.hint,
    }
}
