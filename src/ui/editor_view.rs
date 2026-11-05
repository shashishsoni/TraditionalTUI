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
    let inline_diff = state.inline_diff.as_ref();
    let mut inline_index: usize = 0;

    while y < area.bottom() {
        if line >= line_count {
            break;
        }
        if let Some(diff) = inline_diff {
            let top = diff.top_line as usize;
            if line >= top && inline_index < diff.rows.len() {
                let row = &diff.rows[inline_index];
                inline_index += 1;
                if render_inline_row(
                    buffer,
                    RowLayout {
                        area,
                        y,
                        gutter_width,
                        text_x,
                    },
                    row,
                    inline_index.saturating_sub(1) == diff.selected,
                    focused,
                    state,
                ) {
                    y += 1;
                    let Some(row) = row else { continue };
                    if matches!(
                        row.kind,
                        crate::ai::DiffLineKind::Removed | crate::ai::DiffLineKind::Context
                    ) {
                        line += 1;
                    }
                    continue;
                }
            }
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

/// Horizontal/gutter geometry for one rendered editor row.
#[derive(Debug, Clone, Copy)]
struct RowLayout {
    area: Rect,
    y: u16,
    gutter_width: u16,
    text_x: u16,
}

fn render_inline_row(
    buffer: &mut Buffer,
    layout: RowLayout,
    row: &Option<crate::app::state::InlineDiffRow>,
    selected: bool,
    focused: bool,
    state: &AppState,
) -> bool {
    let RowLayout {
        area,
        y,
        gutter_width,
        text_x,
    } = layout;
    let theme = &state.theme;
    let Some(row) = row else {
        let label = "⋯";
        if text_x > area.x + 1 {
            put(
                buffer,
                area,
                text_x,
                y,
                label,
                Style::default().fg(theme.ui.dim),
            );
        }
        return true;
    };
    let (prefix, fg, bg) = match row.kind {
        crate::ai::DiffLineKind::Removed => ("- ", theme.ui.error, theme.ui.deleted_bg),
        crate::ai::DiffLineKind::Added => ("+ ", theme.ui.success, theme.ui.added_bg),
        crate::ai::DiffLineKind::Context => ("  ", theme.ui.dim, theme.ui.background),
    };
    buffer.set_style(Rect::new(area.x, y, area.width, 1), Style::default().bg(bg));
    for gutter_x in area.x..text_x {
        put(buffer, area, gutter_x, y, " ", Style::default().bg(bg));
    }
    let mut x = area.x;
    let marker = match row.kind {
        crate::ai::DiffLineKind::Removed => "✕",
        crate::ai::DiffLineKind::Added => "+",
        crate::ai::DiffLineKind::Context => " ",
    };
    let digits = gutter_width.saturating_sub(3).max(2) as usize;
    let number = row
        .old_line
        .or(row.new_line)
        .map(|line| format!("{line:>digits$}"))
        .unwrap_or_else(|| " ".repeat(digits));
    let number_style = Style::default().fg(fg).bg(bg);
    for ch in number.chars() {
        if x >= text_x.saturating_sub(2) {
            break;
        }
        put(buffer, area, x, y, &ch.to_string(), number_style);
        x += 1;
    }
    if text_x > area.x + 1 {
        put(
            buffer,
            area,
            text_x.saturating_sub(2),
            y,
            marker,
            Style::default().fg(fg).bg(bg),
        );
        put(
            buffer,
            area,
            text_x.saturating_sub(1),
            y,
            "│",
            Style::default().fg(theme.ui.border).bg(bg),
        );
    }

    x = text_x;
    put(
        buffer,
        area,
        x,
        y,
        &prefix[..1],
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    );
    x += 1;
    for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(row.text.as_str(), true) {
        if x >= area.right() {
            break;
        }
        let style = if row.kind == crate::ai::DiffLineKind::Context {
            Style::default().fg(theme.ui.foreground).bg(bg)
        } else {
            Style::default()
                .fg(theme.ui.foreground)
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        };
        put(buffer, area, x, y, &sanitize_grapheme(grapheme), style);
        x = x.saturating_add(1.max(crate::editor::text::grapheme_width(grapheme, 0, 4) as u16));
    }
    if selected && focused {
        buffer.set_style(
            Rect::new(area.x, y, area.width.min(2), 1),
            Style::default().bg(theme.ui.accent),
        );
    }
    true
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
    buffer.set_style(area, Style::default().bg(theme.ui.tab_bar_bg));

    let hint = "F1 help · Ctrl+Space commands";
    let hint_width = hint.chars().count() as u16;
    let limit = area.right().saturating_sub(hint_width + 2);
    let mut x = area.x;
    let active = state.workspace.active_index();
    let mut drawn = 0usize;
    let mut skipped = 0usize;

    for (index, document) in state.workspace.documents().iter().enumerate() {
        let dirty = if document.dirty { " ●" } else { "" };
        let label = format!(" {}{dirty} ✕ ", document.display_name());
        let width = label.chars().count() as u16;
        if x + width + 1 >= limit {
            if Some(index) == active {
                continue;
            }
            skipped += 1;
            continue;
        }
        let style = if Some(index) == active {
            Style::default()
                .fg(theme.ui.cursor_fg)
                .bg(theme.ui.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.ui.tab_inactive)
                .bg(theme.ui.tab_bar_bg)
        };
        for ch in label.chars() {
            put(buffer, area, x, area.y, &ch.to_string(), style);
            x += 1;
        }
        if Some(index) == active {
            put(
                buffer,
                area,
                x,
                area.y,
                "▂",
                Style::default().fg(theme.ui.accent),
            );
        }
        x += 1;
        drawn += 1;
    }

    if skipped > 0 && x < limit {
        let overflow = format!(" +{skipped} ");
        for ch in overflow.chars() {
            if x >= limit {
                break;
            }
            put(
                buffer,
                area,
                x,
                area.y,
                &ch.to_string(),
                Style::default().fg(theme.ui.dim),
            );
            x += 1;
        }
    }

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
    let _ = drawn;
}

/// Hit-test a click inside the tab bar. Returns `(tab_index, close_clicked)`.
pub fn tab_hit_at(area: Rect, state: &AppState, x: u16, y: u16) -> Option<(usize, bool)> {
    if area.height == 0 || y != area.y || x < area.x {
        return None;
    }
    let hint = "F1 help · Ctrl+Space commands";
    let limit = area.right().saturating_sub(hint.chars().count() as u16 + 2);
    let mut cursor = area.x;
    for (index, document) in state.workspace.documents().iter().enumerate() {
        let dirty = if document.dirty { " ●" } else { "" };
        let label = format!(" {}{dirty} ✕ ", document.display_name());
        let width = label.chars().count() as u16;
        if cursor + width + 1 >= limit {
            if Some(index) == state.workspace.active_index() {
                let _ = document;
                continue;
            }
            break;
        }
        if x >= cursor && x < cursor + width {
            return Some((index, x + 2 >= cursor + width));
        }
        cursor += width + 1;
        if cursor >= limit {
            break;
        }
    }
    None
}
