//! Editor viewport renderer with grapheme-safe cell blitting.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Clear;
use unicode_segmentation::UnicodeSegmentation;

use crate::app::state::{AppState, EditMode, FocusTarget};
use crate::editor::syntax::{HighlightState, TokenKind, highlight_line};
use crate::editor::text::grapheme_width;

/// Render the active buffer into `area` (below the tab bar).
pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    frame.render_widget(Clear, area);
    let buffer = frame.buffer_mut();
    buffer.set_style(area, Style::default().bg(theme.ui.background));

    let Some(document) = state.active_doc() else {
        put_text(
            buffer,
            area,
            area.y,
            "  No buffer open — Ctrl+P to open a file",
            theme.ui.dim,
        );
        return;
    };

    if document.rope.len_chars() == 0 {
        render_welcome(buffer, area, state);
        return;
    }

    let focused = state.focus == FocusTarget::Editor;
    let line_count = document.line_count().max(1);
    let digits = line_count.to_string().len() as u16;
    let gutter_width = digits + 3;
    let text_x = area.x + gutter_width;
    let tab_width = state.config.editor.tab_width.max(1);
    let cursor_line = document.cursor_line_col().0;
    let git_marks = git_mark_map(&document.git_diff.marks, &state.theme);

    let mut line = document.top_line.min(line_count.saturating_sub(1));
    let mut y = area.y;
    let mut highlight = HighlightState::default();

    while y < area.bottom() {
        if line >= line_count {
            break;
        }
        while line + 1 < line_count && document.folds.is_hidden(line) {
            line += 1;
        }
        if document.folds.is_hidden(line) {
            break;
        }

        let line_start = document.rope.line_to_char(line);
        let text = document.line_text_at(line);
        let spans = highlight_line(&text, &document.language_id, &mut highlight);
        let char_count = text.chars().count();
        let mut kinds: Vec<Option<TokenKind>> = vec![None; char_count];
        for span in spans {
            for kind in kinds
                .iter_mut()
                .take(span.end.min(char_count))
                .skip(span.start)
            {
                *kind = Some(span.kind);
            }
        }

        if line == cursor_line {
            buffer.set_style(
                Rect::new(area.x, y, area.width, 1),
                Style::default().bg(theme.ui.status_bg),
            );
        }
        for gutter_x in area.x..text_x {
            put(
                buffer,
                area,
                gutter_x,
                y,
                " ",
                Style::default().bg(theme.ui.status_bg),
            );
        }

        let number_style = if line == cursor_line {
            Style::default()
                .fg(theme.ui.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.ui.dim)
        };
        let number = format!("{:>width$}", line + 1, width = digits as usize);
        let mut x = area.x;
        for ch in number.chars() {
            if x >= text_x {
                break;
            }
            put(buffer, area, x, y, &ch.to_string(), number_style);
            x += 1;
        }
        while x < text_x.saturating_sub(2) {
            put(buffer, area, x, y, " ", number_style);
            x += 1;
        }
        if text_x > area.x + 1 {
            if let Some(marker) = git_marks.get(&line) {
                put(
                    buffer,
                    area,
                    text_x.saturating_sub(2),
                    y,
                    marker.0,
                    Style::default().fg(marker.1),
                );
            } else {
                put(buffer, area, text_x.saturating_sub(2), y, " ", number_style);
            }
        }
        if text_x > area.x {
            put(
                buffer,
                area,
                text_x.saturating_sub(1),
                y,
                "│",
                Style::default().fg(theme.ui.border),
            );
        }

        x = text_x;
        let mut char_pos = 0usize;
        let mut visual = 0usize;
        let left_col = document.left_col;

        for grapheme in text.graphemes(true) {
            let grapheme_chars = grapheme.chars().count();
            let width = grapheme_width(grapheme, visual, tab_width);
            if visual + width <= left_col {
                visual += width;
                char_pos += grapheme_chars;
                continue;
            }

            let global = line_start + char_pos;
            let mut style = token_style(kinds.get(char_pos).copied().flatten(), state);
            if document
                .cursors
                .iter()
                .any(|cursor| cursor.range().contains(&global))
            {
                style = style.bg(theme.ui.selection);
            }
            if focused && document.cursors.iter().any(|cursor| cursor.head == global) {
                style = style
                    .bg(theme.ui.cursor)
                    .fg(theme.ui.cursor_fg)
                    .add_modifier(Modifier::BOLD);
            }

            if grapheme == "\t" {
                for _ in 0..width {
                    if x >= area.right() {
                        break;
                    }
                    put(buffer, area, x, y, " ", style);
                    x += 1;
                }
            } else {
                if width as u16 >= 2 && x + 1 >= area.right() {
                    put(buffer, area, x, y, "›", Style::default().fg(theme.ui.dim));
                    break;
                }
                let symbol = sanitize_grapheme(grapheme);
                put(buffer, area, x, y, &symbol, style);
                x = x.saturating_add(width as u16);
            }
            visual += width;
            char_pos += grapheme_chars;
        }

        // Caret at end-of-line has no grapheme to paint on.
        if focused && line == cursor_line && x < area.right() {
            let head = document.cursors.primary().head;
            if head == line_start + char_count {
                put(
                    buffer,
                    area,
                    x,
                    y,
                    " ",
                    Style::default().bg(theme.ui.cursor).fg(theme.ui.cursor_fg),
                );
            }
        }

        line += 1;
        y += 1;
    }
}

fn render_welcome(buffer: &mut Buffer, area: Rect, state: &AppState) {
    let theme = &state.theme;
    let hints: [(&str, &str); 12] = [
        ("Ctrl+Space", "command palette"),
        ("Ctrl+P", "go to file"),
        ("Ctrl+T", "open terminal"),
        ("Alt+F", "project search"),
        ("Ctrl+G", "connect ACP agent"),
        ("Ctrl+O", "open configuration"),
        ("Alt+O", "open another folder"),
        ("Alt+B", "switch git branch"),
        ("F5/F6/F7", "explorer / git / search"),
        ("F8", "review AI diff"),
        ("F1", "keymap help"),
        ("Esc", "normal mode"),
    ];
    let key_width = hints
        .iter()
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(0)
        + 3;
    let body_width = key_width
        + hints
            .iter()
            .map(|(_, description)| description.chars().count())
            .max()
            .unwrap_or(0);
    let block_width = body_width.max(42).min(area.width as usize);
    let subtitle = format!("TradinationalTUI · {}", state.workspace_name());
    let mode_text = match state.mode {
        EditMode::Insert => "INSERT  type to edit · Esc for normal mode",
        _ => "NORMAL  i to insert · F1 for help",
    };
    let total_lines = 4 + hints.len();
    let start_y = area.y + area.height.saturating_sub(total_lines as u16) / 2;

    draw_centered(
        buffer,
        area,
        start_y,
        "ttui",
        Style::default()
            .fg(theme.ui.accent)
            .add_modifier(Modifier::BOLD),
    );
    draw_centered(
        buffer,
        area,
        start_y + 1,
        &subtitle,
        Style::default().fg(theme.ui.dim),
    );
    draw_centered(
        buffer,
        area,
        start_y + 2,
        mode_text,
        Style::default().fg(if state.mode == EditMode::Insert {
            theme.ui.success
        } else {
            theme.ui.warning
        }),
    );

    let start_x = area.x + area.width.saturating_sub(block_width as u16) / 2;
    let hints_y = start_y + 4;
    for (offset, (key, description)) in hints.iter().enumerate() {
        let y = hints_y + offset as u16;
        if y >= area.bottom() {
            break;
        }
        let key_style = Style::default()
            .fg(theme.ui.accent)
            .add_modifier(Modifier::BOLD);
        let description_style = Style::default().fg(theme.ui.foreground);
        for (index, ch) in key.chars().enumerate() {
            put(
                buffer,
                area,
                start_x + index as u16,
                y,
                &ch.to_string(),
                key_style,
            );
        }
        for (index, ch) in description.chars().enumerate() {
            put(
                buffer,
                area,
                start_x + key_width as u16 + index as u16,
                y,
                &ch.to_string(),
                description_style,
            );
        }
    }
}

fn draw_centered(buffer: &mut Buffer, area: Rect, y: u16, text: &str, style: Style) {
    let width = text.chars().count() as u16;
    let x = area.x + area.width.saturating_sub(width) / 2;
    for (index, ch) in text.chars().enumerate() {
        put(buffer, area, x + index as u16, y, &ch.to_string(), style);
    }
}

fn token_style(kind: Option<TokenKind>, state: &AppState) -> Style {
    let theme = &state.theme;
    let color = match kind {
        None => theme.ui.foreground,
        Some(TokenKind::Keyword) => theme.syntax.keyword,
        Some(TokenKind::Type) => theme.syntax.type_name,
        Some(TokenKind::Function) => theme.syntax.function,
        Some(TokenKind::String) => theme.syntax.string,
        Some(TokenKind::Comment) => theme.syntax.comment,
        Some(TokenKind::Number) => theme.syntax.number,
        Some(TokenKind::Constant) => theme.syntax.constant,
        Some(TokenKind::Operator) => theme.syntax.operator,
    };
    Style::default().fg(color)
}

fn sanitize_grapheme(grapheme: &str) -> String {
    if grapheme.chars().any(|ch| ch.is_control()) {
        "·".to_string()
    } else {
        grapheme.to_string()
    }
}

fn put_text(buffer: &mut Buffer, area: Rect, y: u16, text: &str, color: ratatui::style::Color) {
    for (index, ch) in text.chars().enumerate() {
        let x = area.x + index as u16;
        if x >= area.right() {
            break;
        }
        put(
            buffer,
            area,
            x,
            y,
            &ch.to_string(),
            Style::default().fg(color),
        );
    }
}

fn put(buffer: &mut Buffer, area: Rect, x: u16, y: u16, symbol: &str, style: Style) {
    if !area.contains(ratatui::layout::Position::new(x, y)) {
        return;
    }
    if let Some(cell) = buffer.cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

fn git_mark_map(
    marks: &[(usize, crate::editor::GutterMark)],
    theme: &crate::ui::theme::Theme,
) -> std::collections::HashMap<usize, (&'static str, ratatui::style::Color)> {
    use crate::editor::GutterMark;
    let mut map = std::collections::HashMap::new();
    for (line, kind) in marks {
        let (glyph, color) = match kind {
            GutterMark::Added => ("+", theme.ui.success),
            GutterMark::Modified => ("~", theme.ui.warning),
            GutterMark::Deleted => ("_", theme.ui.error),
            GutterMark::Conflict => ("!", theme.ui.error),
        };
        map.insert(*line, (glyph, color));
    }
    map
}

/// Render the tab bar across the top of the editor zone.
pub fn render_tabs(frame: &mut Frame, area: Rect, state: &AppState) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let theme = &state.theme;
    frame.render_widget(Clear, area);
    let buffer = frame.buffer_mut();
    buffer.set_style(area, Style::default().bg(theme.ui.status_bg));
    let mut x = area.x + 1;
    let active = state.workspace.active_index();
    let hint = "F1 help · Ctrl+Space commands ";
    let reserved = hint.chars().count() as u16;
    let limit = area.right().saturating_sub(reserved);

    for (index, document) in state.workspace.documents().iter().enumerate() {
        let dirty = if document.dirty { " ●" } else { "" };
        let label = format!(" {}{dirty} ✕ ", document.display_name());
        let mut style = if Some(index) == active {
            Style::default()
                .fg(theme.ui.cursor_fg)
                .bg(theme.ui.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.ui.tab_inactive)
                .bg(theme.ui.status_bg)
        };
        if Some(index) == active && state.focus == FocusTarget::Editor {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        let mut wrote = false;
        for ch in label.chars() {
            if x >= limit {
                break;
            }
            put(buffer, area, x, area.y, &ch.to_string(), style);
            x += 1;
            wrote = true;
        }
        if x >= limit {
            break;
        }
        if !wrote {
            break;
        }
        put(
            buffer,
            area,
            x,
            area.y,
            "│",
            Style::default().fg(theme.ui.border),
        );
        x += 1;
    }

    let hint_width = hint.chars().count() as u16;
    if area.width > hint_width + 4 {
        let hint_x = area.right().saturating_sub(hint_width);
        for (index, ch) in hint.chars().enumerate() {
            put(
                buffer,
                area,
                hint_x + index as u16,
                area.y,
                &ch.to_string(),
                Style::default().fg(theme.ui.dim),
            );
        }
    }
}

/// Hit-test a click inside the tab bar. Returns `(tab_index, close_clicked)`.
pub fn tab_hit_at(area: Rect, state: &AppState, x: u16, y: u16) -> Option<(usize, bool)> {
    if area.height == 0 || y != area.y || x < area.x + 1 {
        return None;
    }
    let hint = "F1 help · Ctrl+Space commands ";
    let limit = area.right().saturating_sub(hint.chars().count() as u16);
    let mut cursor = area.x + 1;
    for (index, document) in state.workspace.documents().iter().enumerate() {
        let dirty = if document.dirty { " ●" } else { "" };
        let label = format!(" {}{dirty} ✕ ", document.display_name());
        let width = label.chars().count() as u16;
        if x >= cursor && x < cursor + width {
            if x + 1 >= limit {
                return None;
            }
            return Some((index, x + 2 >= cursor + width));
        }
        cursor += width + 1;
        if cursor >= limit {
            break;
        }
    }
    None
}
