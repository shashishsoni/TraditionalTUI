//! One-line status bar.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::{AppState, EditMode};
use crate::ui::bottom_panel::message_color;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    let background = Style::default()
        .bg(theme.ui.status_bg)
        .fg(theme.ui.status_fg);
    frame.buffer_mut().set_style(area, background);

    let mode_color = match state.mode {
        EditMode::Normal => theme.ui.accent,
        EditMode::Insert => theme.ui.success,
        EditMode::Visual => theme.ui.warning,
        EditMode::Command => theme.ui.hint,
    };
    let mut spans = vec![Span::styled(
        format!(" {} ", state.mode.label()),
        Style::default()
            .fg(theme.ui.cursor_fg)
            .bg(mode_color)
            .add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::styled("│", Style::default().fg(theme.ui.border)));

    if state.mode == EditMode::Command {
        spans.push(Span::styled(
            format!(":{}", state.command_buffer),
            Style::default().fg(theme.ui.hint),
        ));
    } else if let Some(message) = &state.status_message {
        spans.push(Span::styled(
            format!(" {} ", message.text),
            Style::default().fg(message_color(message.kind, state)),
        ));
    } else {
        let name = state
            .active_doc()
            .map(|document| {
                format!(
                    "{}{}",
                    document.display_name(),
                    if document.dirty { " *" } else { "" }
                )
            })
            .unwrap_or_else(|| "scratch".to_string());
        spans.push(Span::styled(
            format!(" {name} "),
            Style::default().fg(theme.ui.foreground),
        ));

        if let Some(git) = &state.git {
            if let Some(branch) = git.branch() {
                spans.push(Span::styled(
                    format!("│ ⑂ {branch} "),
                    Style::default().fg(theme.ui.hint),
                ));
            }
        }

        let (errors, warnings) = state.lsp.diagnostics().counts();
        spans.push(Span::styled(
            format!("│ E:{errors} W:{warnings} "),
            Style::default().fg(if errors > 0 {
                theme.ui.error
            } else if warnings > 0 {
                theme.ui.warning
            } else {
                theme.ui.dim
            }),
        ));

        if state.acp.agent_name().is_some() {
            spans.push(Span::styled(
                format!(
                    "│ ACP: {} ({}) ",
                    state.acp.agent_name().unwrap_or("agent"),
                    state.acp.state().label()
                ),
                Style::default().fg(theme.ui.success),
            ));
        }
    }

    if let Some(pending) = state.pending {
        spans.push(Span::styled(
            format!("│ {pending} … "),
            Style::default().fg(theme.ui.warning),
        ));
    }

    if let Some(document) = state.active_doc() {
        let (line, column) = document.status_position();
        let right = format!(
            " {}  {}  Ln {line}, Col {column} ",
            document.encoding.label(),
            document.line_ending.label()
        );
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let padding = (area.width as usize).saturating_sub(used + right.chars().count());
        if padding > 0 {
            spans.push(Span::raw(" ".repeat(padding)));
        }
        spans.push(Span::styled(right, Style::default().fg(theme.ui.dim)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)).style(background), area);
}
