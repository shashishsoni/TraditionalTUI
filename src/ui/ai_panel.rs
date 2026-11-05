//! Right-hand AI / ACP assistant sidebar.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::ai::{AiState, ChatRole};
use crate::app::state::{AppState, FocusTarget};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    let focused = state.focus == FocusTarget::Ai;
    let block = Block::bordered()
        .border_style(Style::default().fg(if focused {
            theme.ui.border_focused
        } else {
            theme.ui.border
        }))
        .title(Line::from(Span::styled(
            " AI Assistant ",
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

    let mut lines: Vec<Line> = Vec::new();
    let ai_status = match state.ai.state() {
        AiState::Disabled => "off",
        AiState::Idle => "ready",
        AiState::Streaming => "streaming",
        AiState::Error => "error",
    };
    match state.ai.provider_name() {
        Some(provider) => lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.ui.success)),
            Span::styled("BYOK  ", Style::default().fg(theme.ui.dim)),
            Span::styled(
                format!("{provider} · {}", state.ai.model()),
                Style::default().fg(theme.ui.hint),
            ),
            Span::styled(
                format!("  [{ai_status}]"),
                Style::default().fg(theme.ui.dim),
            ),
        ])),
        None => lines.push(Line::from(vec![
            Span::styled("○ ", Style::default().fg(theme.ui.dim)),
            Span::styled("BYOK  ", Style::default().fg(theme.ui.dim)),
            Span::styled("not configured", Style::default().fg(theme.ui.foreground)),
        ])),
    }

    if state.acp.is_connected() || state.acp.agent_name().is_some() {
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(theme.ui.success)),
            Span::styled("ACP   ", Style::default().fg(theme.ui.dim)),
            Span::styled(
                state.acp.agent_name().unwrap_or("agent").to_string(),
                Style::default().fg(theme.ui.success),
            ),
            Span::styled(
                format!("  [{}]", state.acp.state().label()),
                Style::default().fg(theme.ui.dim),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("○ ", Style::default().fg(theme.ui.dim)),
            Span::styled("ACP   ", Style::default().fg(theme.ui.dim)),
            Span::styled("not connected", Style::default().fg(theme.ui.foreground)),
        ]));
    }

    if focused {
        lines.push(Line::from(Span::styled(
            " i ask · a agents · x cancel · c clear",
            Style::default().fg(theme.ui.dim),
        )));
    }
    lines.push(Line::from(""));

    let thread = state.ai.thread();
    if thread.is_empty() {
        if state.ai.is_configured() {
            lines.push(Line::from(Span::styled(
                " Ask about the current file",
                Style::default().fg(theme.ui.foreground),
            )));
            lines.push(Line::from(vec![
                Span::styled(" i  ", Style::default().fg(theme.ui.accent)),
                Span::styled("open the prompt", Style::default().fg(theme.ui.dim)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                " Start an AI session",
                Style::default()
                    .fg(theme.ui.foreground)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("Ctrl+G ", Style::default().fg(theme.ui.accent)),
                Span::styled("connect an ACP agent", Style::default().fg(theme.ui.dim)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Ctrl+O ", Style::default().fg(theme.ui.accent)),
                Span::styled("open config / BYOK keys", Style::default().fg(theme.ui.dim)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(" i   ", Style::default().fg(theme.ui.accent)),
                Span::styled("ask the assistant", Style::default().fg(theme.ui.dim)),
            ]));
        }

        if !state.acp.is_connected() {
            let agents = state.acp.available_agents();
            if agents.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " No ACP agents found on PATH.",
                    Style::default().fg(theme.ui.dim),
                )));
            } else {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Available agents",
                    Style::default().fg(theme.ui.dim),
                )));
                for profile in agents.iter().take(6) {
                    lines.push(Line::from(vec![
                        Span::styled("  · ", Style::default().fg(theme.ui.dim)),
                        Span::styled(
                            profile.name.clone(),
                            Style::default().fg(theme.ui.foreground),
                        ),
                        Span::styled(
                            format!("  {}", profile.id),
                            Style::default().fg(theme.ui.dim),
                        ),
                    ]));
                }
            }
        }
    } else {
        let width = content.width.saturating_sub(3).max(8) as usize;
        for entry in thread.entries() {
            let (label, color) = match entry.role {
                ChatRole::User => ("You", theme.ui.accent),
                ChatRole::Assistant => ("AI", theme.ui.success),
            };
            lines.push(Line::from(Span::styled(
                label.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            for wrapped in wrap(&entry.content, width) {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(wrapped, Style::default().fg(theme.ui.foreground)),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    if let Some(error) = state.acp.last_error() {
        lines.push(Line::from(Span::styled(
            format!("ACP: {error}"),
            Style::default().fg(theme.ui.error),
        )));
    }

    let height = content.height as usize;
    let start = lines.len().saturating_sub(height);
    let visible: Vec<Line> = lines.into_iter().skip(start).collect();
    frame.render_widget(Paragraph::new(visible), content);
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            if word.chars().count() > width {
                for chunk in chunk_chars(word, width) {
                    lines.push(chunk);
                }
            } else {
                current.push_str(word);
            }
        } else if current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn chunk_chars(text: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= width {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_lines() {
        let lines = wrap("alpha beta gamma delta", 10);
        assert!(lines.iter().all(|line| line.chars().count() <= 10));
        assert!(!lines.is_empty());
    }

    #[test]
    fn long_words_are_chunked() {
        let lines = wrap("abcdefghijklmnop", 5);
        assert!(lines.iter().all(|line| line.chars().count() <= 5));
    }
}
