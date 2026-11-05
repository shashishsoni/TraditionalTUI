//! Bottom panel: integrated terminal, problems, output and ACP logs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::state::{AppState, BottomTab, FocusTarget, MessageKind};
use crate::lsp::Severity;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    let focused = matches!(state.focus, FocusTarget::Bottom(_));
    let block = Block::bordered().border_style(Style::default().fg(if focused {
        theme.ui.border_focused
    } else {
        theme.ui.border
    }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame
        .buffer_mut()
        .set_style(inner, Style::default().bg(theme.ui.panel_bg));
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let tabs_area = Rect::new(inner.x + 1, inner.y, inner.width.saturating_sub(2), 1);
    let titles: Vec<Line> = BottomTab::ALL
        .iter()
        .map(|tab| Line::from(Span::styled(format!(" {} ", tab.label()), Style::default())))
        .collect();
    frame.render_widget(
        Tabs::new(titles)
            .select(state.bottom_tab.index())
            .style(Style::default().fg(theme.ui.dim).bg(theme.ui.panel_bg))
            .highlight_style(
                Style::default()
                    .fg(theme.ui.cursor_fg)
                    .bg(theme.ui.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(Span::styled(" ", Style::default().fg(theme.ui.dim))),
        tabs_area,
    );

    let hint = if state.bottom_tab == BottomTab::Terminal {
        "Ctrl+T new · Ctrl+Space navigate "
    } else {
        "1-4 switch · Ctrl+T terminal "
    };
    let hint_width = hint.chars().count() as u16;
    if tabs_area.width > hint_width + 24 {
        let hint_x = tabs_area.right().saturating_sub(hint_width);
        let buffer = frame.buffer_mut();
        for (index, ch) in hint.chars().enumerate() {
            if let Some(cell) = buffer.cell_mut((hint_x + index as u16, tabs_area.y)) {
                cell.set_symbol(&ch.to_string());
                cell.set_style(Style::default().fg(theme.ui.dim));
            }
        }
    }

    let content = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(1),
    );
    if content.height == 0 || content.width == 0 {
        return;
    }

    match state.bottom_tab {
        BottomTab::Terminal => render_terminal(frame, content, state, focused),
        BottomTab::Problems => render_problems(frame, content, state),
        BottomTab::Output => {
            let lines: Vec<Line> = state
                .output
                .iter()
                .rev()
                .take(content.height as usize)
                .rev()
                .map(|line| Line::from(Span::raw(line.clone())))
                .collect();
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().fg(theme.ui.dim)),
                content,
            );
        }
        BottomTab::AcpLogs => {
            let trace = state.acp.supervisor().trace();
            let lines: Vec<Line> = trace
                .lines()
                .iter()
                .rev()
                .take(content.height as usize)
                .rev()
                .map(|line| Line::from(Span::raw(line.clone())))
                .collect();
            let empty = lines.is_empty();
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().fg(theme.ui.dim)),
                content,
            );
            if empty {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "no ACP traffic yet",
                        Style::default().fg(theme.ui.dim),
                    ))),
                    content,
                );
            }
        }
    }
}

fn render_terminal(frame: &mut Frame, area: Rect, state: &AppState, focused: bool) {
    let theme = &state.theme;
    let Some(session) = state.terminal.active() else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No shell running",
                    Style::default().fg(theme.ui.foreground),
                )),
                Line::from(Span::styled(
                    "Press Enter or Ctrl+T to start one",
                    Style::default().fg(theme.ui.dim),
                )),
            ]),
            area,
        );
        return;
    };

    let screen = session.screen();
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let buffer = frame.buffer_mut();
    let visible_rows = rows.min(area.height);
    let visible_cols = cols.min(area.width);

    for row in 0..visible_rows {
        for col in 0..visible_cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let mut symbol = cell.contents();
            if symbol.is_empty() {
                symbol = " ".to_string();
            } else if symbol.chars().any(|ch| ch.is_control()) {
                symbol = "·".to_string();
            }
            let mut foreground = vt_color(cell.fgcolor(), theme.ui.foreground);
            let mut background = vt_color(cell.bgcolor(), theme.ui.background);
            let mut modifier = Modifier::empty();
            if cell.bold() {
                modifier |= Modifier::BOLD;
            }
            if cell.italic() {
                modifier |= Modifier::ITALIC;
            }
            if cell.underline() {
                modifier |= Modifier::UNDERLINED;
            }
            if cell.inverse() {
                std::mem::swap(&mut foreground, &mut background);
            }
            if focused && row == cursor_row && col == cursor_col {
                std::mem::swap(&mut foreground, &mut background);
                modifier |= Modifier::BOLD;
            }
            if let Some(target) = buffer.cell_mut((area.x + col, area.y + row)) {
                target.set_symbol(&symbol);
                target.set_style(
                    Style::default()
                        .fg(foreground)
                        .bg(background)
                        .add_modifier(modifier),
                );
            }
        }
    }
}

fn render_problems(frame: &mut Frame, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let diagnostics: Vec<&crate::lsp::DiagnosticEntry> = state.lsp.diagnostics().all().collect();
    if diagnostics.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No diagnostics",
                Style::default().fg(theme.ui.dim),
            ))),
            area,
        );
        return;
    }
    let lines: Vec<Line> = diagnostics
        .iter()
        .take(area.height as usize)
        .map(|entry| {
            let color = match entry.severity {
                Severity::Error => theme.ui.error,
                Severity::Warning => theme.ui.warning,
                Severity::Information => theme.ui.hint,
                Severity::Hint => theme.ui.dim,
            };
            Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.severity.label()),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}:{}: ", entry.uri, entry.line + 1),
                    Style::default().fg(theme.ui.dim),
                ),
                Span::raw(entry.message.clone()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn vt_color(color: vt100::Color, fallback: ratatui::style::Color) -> ratatui::style::Color {
    match color {
        vt100::Color::Default => fallback,
        vt100::Color::Idx(index) => ratatui::style::Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => ratatui::style::Color::Rgb(r, g, b),
    }
}

/// Compact color hint for status bar severity counts.
pub fn severity_color(severity: Severity, state: &AppState) -> ratatui::style::Color {
    match severity {
        Severity::Error => state.theme.ui.error,
        Severity::Warning => state.theme.ui.warning,
        Severity::Information | Severity::Hint => state.theme.ui.hint,
    }
}

/// Message kind color helper shared by the status bar.
pub fn message_color(kind: MessageKind, state: &AppState) -> ratatui::style::Color {
    match kind {
        MessageKind::Info => state.theme.ui.success,
        MessageKind::Warning => state.theme.ui.warning,
        MessageKind::Error => state.theme.ui.error,
    }
}
