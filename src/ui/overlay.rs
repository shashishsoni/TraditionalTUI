//! Modal overlays: palette, finders, search, help, themes and permissions.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::acp::PermissionOptionKind;
use crate::app::state::{AppState, ModalKind, ModalState};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(modal) = &state.active_modal else {
        return;
    };
    let theme = &state.theme;
    let width = area.width.saturating_sub(6).min(88);
    let height = match modal.kind {
        ModalKind::Help => area.height.saturating_sub(6).min(24),
        ModalKind::PermissionPrompt => area.height.saturating_sub(6).min(18),
        _ => area.height.saturating_sub(6).min(20),
    };
    if width < 20 || height < 6 {
        return;
    }
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_style(Style::default().fg(theme.ui.border_focused))
        .title(Line::from(Span::styled(
            format!(" {} ", modal.kind.title()),
            Style::default()
                .fg(theme.ui.accent)
                .add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame
        .buffer_mut()
        .set_style(inner, Style::default().bg(theme.ui.panel_bg));

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
    let body = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    );

    let footer_text = match modal.kind {
        ModalKind::PermissionPrompt => " y allow · a allow always · n reject · Esc cancel",
        ModalKind::GlobalSearch => " Enter search · ↑↓ select · Esc close",
        ModalKind::AiPrompt => " Enter send · Esc cancel",
        ModalKind::Rename | ModalKind::NewFile => " Enter confirm · Esc cancel",
        _ => " ↑↓ select · Enter confirm · Esc close",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer_text,
            Style::default().fg(theme.ui.dim),
        ))),
        footer,
    );

    match modal.kind {
        ModalKind::PermissionPrompt => render_permission(frame, body, modal, state),
        ModalKind::Help => render_list(frame, body, modal, state),
        _ => {
            let mut y = body.y;
            if modal.kind.has_input() {
                let prefix = match modal.kind {
                    ModalKind::GlobalSearch => "regex ",
                    ModalKind::BufferSearch => "/",
                    ModalKind::AiPrompt => "ask ",
                    ModalKind::Rename => "rename ",
                    ModalKind::NewFile => "new file ",
                    _ => "> ",
                };
                let line = Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(theme.ui.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        modal.query.clone(),
                        Style::default().fg(theme.ui.foreground),
                    ),
                    Span::styled("_", Style::default().fg(theme.ui.cursor)),
                ]);
                frame.render_widget(
                    Paragraph::new(line).style(Style::default().bg(theme.ui.panel_bg)),
                    Rect::new(body.x, y, body.width, 1),
                );
                y += 1;
                if modal.busy {
                    frame.render_widget(
                        Paragraph::new(Line::from(Span::styled(
                            "searching…",
                            Style::default().fg(theme.ui.dim),
                        ))),
                        Rect::new(body.x, y, body.width, 1),
                    );
                    return;
                }
                let list_area = Rect::new(
                    body.x,
                    y,
                    body.width,
                    body.height.saturating_sub(y.saturating_sub(body.y)),
                );
                render_list(frame, list_area, modal, state);
            } else {
                render_list(frame, body, modal, state);
            }
        }
    }
}

fn render_list(frame: &mut Frame, area: Rect, modal: &ModalState, state: &AppState) {
    if area.height == 0 {
        return;
    }
    let theme = &state.theme;
    if modal.entries.is_empty() {
        let hint = match modal.kind {
            ModalKind::GlobalSearch => " type a regex and press Enter",
            ModalKind::CommandPalette => " no matching commands",
            ModalKind::FileFinder => " no matching files (indexing…)",
            ModalKind::ThemeSelector => " no themes",
            ModalKind::AgentPicker => " no ACP agents found — F4 opens config",
            _ => "",
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                hint,
                Style::default().fg(theme.ui.dim),
            ))),
            area,
        );
        return;
    }

    let height = area.height as usize;
    let start = modal.selected.saturating_sub(height / 2);
    let mut lines = Vec::new();
    for (index, entry) in modal.entries.iter().enumerate().skip(start).take(height) {
        let selected = index == modal.selected;
        let marker = if selected { "▸ " } else { "  " };
        let mut line = Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.ui.accent)),
            Span::styled(
                entry.label.clone(),
                Style::default().fg(theme.ui.foreground),
            ),
            Span::styled(
                format!("  {}", entry.detail),
                Style::default().fg(theme.ui.dim),
            ),
        ]);
        if selected {
            line = line.style(Style::default().bg(theme.ui.selection));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_permission(frame: &mut Frame, area: Rect, modal: &ModalState, state: &AppState) {
    let theme = &state.theme;
    let Some(prompt) = &modal.permission else {
        return;
    };
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        prompt.summary(),
        Style::default()
            .fg(theme.ui.warning)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for option in &prompt.options {
        let key = match &option.kind {
            PermissionOptionKind::AllowOnce => "y",
            PermissionOptionKind::AllowAlways => "a",
            PermissionOptionKind::RejectOnce | PermissionOptionKind::RejectAlways => "n",
            PermissionOptionKind::Unknown(_) => "?",
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("[{key}] "),
                Style::default()
                    .fg(theme.ui.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                option.name.clone(),
                Style::default().fg(theme.ui.foreground),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
