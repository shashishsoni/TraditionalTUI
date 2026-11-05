//! One-line status bar with clickable git/branch segment.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::{AppState, EditMode, FocusTarget};
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

    let (mut spans, _) = compose(state);
    if let Some(document) = state.active_doc() {
        let (line, column) = document.status_position();
        let scroll = if document.left_col > 0 {
            format!("↔{}  ", document.left_col)
        } else {
            String::new()
        };
        let right = format!(
            " {}  {}  {scroll}Ln {line}, Col {column} ",
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

/// Character range of the git segment inside the status bar, if visible.
pub fn git_range(state: &AppState) -> Option<(u16, u16)> {
    compose(state).1
}

fn compose(state: &AppState) -> (Vec<Span<'static>>, Option<(u16, u16)>) {
    let theme = &state.theme;
    let mode_color = match state.mode {
        EditMode::Normal => theme.ui.accent,
        EditMode::Insert => theme.ui.success,
        EditMode::Visual => theme.ui.warning,
        EditMode::Command => theme.ui.hint,
    };
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        format!(" {} ", state.mode.label()),
        Style::default()
            .fg(theme.ui.cursor_fg)
            .bg(mode_color)
            .add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::styled("│", Style::default().fg(theme.ui.border)));
    let mut git_range = None;

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
        spans.push(Span::styled(
            format!("│ {} ", focus_label(state)),
            Style::default().fg(theme.ui.dim),
        ));

        let start = spans
            .iter()
            .map(|span| span.content.chars().count() as u16)
            .sum::<u16>();
        let (git_text, git_color) = match &state.git {
            Some(git) => {
                let count = state.source_control_files().len();
                match git.branch() {
                    Some(branch) if !git.is_detached() => {
                        (format!("│ ⑂ {branch} {count}Δ "), theme.ui.hint)
                    }
                    Some(branch) => (format!("│ ⑂⌖ {branch} {count}Δ "), theme.ui.warning),
                    None => ("│ ◇ open folder ".to_string(), theme.ui.dim),
                }
            }
            None => ("│ ◇ open folder ".to_string(), theme.ui.dim),
        };
        let end = start + git_text.chars().count() as u16;
        spans.push(Span::styled(git_text, Style::default().fg(git_color)));
        git_range = Some((start, end));

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

        let acp = match state.acp.agent_name() {
            Some(agent) => format!("ACP {agent} · {}", state.acp.state().label()),
            None => "ACP off".to_string(),
        };
        let ai = match state.ai.provider_name() {
            Some(provider) => format!("AI {provider}"),
            None => "AI off".to_string(),
        };
        spans.push(Span::styled(
            format!("│ {acp} "),
            Style::default().fg(if state.acp.is_connected() {
                theme.ui.success
            } else {
                theme.ui.dim
            }),
        ));
        spans.push(Span::styled(
            format!("│ {ai} "),
            Style::default().fg(if state.ai.is_configured() {
                theme.ui.hint
            } else {
                theme.ui.dim
            }),
        ));
    }

    if let Some(pending) = state.pending {
        spans.push(Span::styled(
            format!("│ {pending} … "),
            Style::default().fg(theme.ui.warning),
        ));
    }

    (spans, git_range)
}

fn focus_label(state: &AppState) -> &'static str {
    match state.focus {
        FocusTarget::Explorer => match state.sidebar_view {
            crate::app::state::SidebarView::Explorer => "EXPLORER",
            crate::app::state::SidebarView::SourceControl => "SOURCE CONTROL",
            crate::app::state::SidebarView::Search => "SEARCH",
        },
        FocusTarget::Editor => "EDITOR",
        FocusTarget::Bottom(crate::app::state::BottomTab::Terminal) => "TERMINAL",
        FocusTarget::Bottom(crate::app::state::BottomTab::Problems) => "PROBLEMS",
        FocusTarget::Bottom(crate::app::state::BottomTab::Output) => "OUTPUT",
        FocusTarget::Bottom(crate::app::state::BottomTab::AcpLogs) => "ACP LOGS",
        FocusTarget::Ai => "AI",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_composition_exposes_git_range() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::for_testing(dir.path());
        let (spans, range) = compose(&state);
        assert!(!spans.is_empty());
        let (start, end) = range.expect("git segment");
        assert!(end > start);
    }
}
