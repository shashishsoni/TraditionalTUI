//! Lightweight lexical syntax highlighting.
//!
//! The primary shipping highlighters are intentionally dependency-free
//! lexical scanners: they cover comments, strings, numbers, keywords, types
//! and function calls for the supported languages. The renderer consumes
//! [`Span`] values in character coordinates, so adding a Tree-sitter backend
//! later only requires producing the same spans.

use std::path::Path;

/// Semantic token category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    Type,
    Function,
    String,
    Comment,
    Number,
    Constant,
    Operator,
}

/// A token range in character offsets within one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Multi-line scanner state carried between lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HighlightState {
    pub in_block_comment: bool,
}

/// Map a file extension to a language identifier.
pub fn language_id_for_path(path: &Path) -> &'static str {
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        let special = match name {
            "Dockerfile" | "Containerfile" => "dockerfile",
            "Makefile" | "makefile" | "GNUmakefile" => "make",
            "Cargo.lock" => "toml",
            _ => "",
        };
        if !special.is_empty() {
            return special;
        }
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    language_id_for_extension(&extension)
}

/// Map a lower-case extension to a language identifier.
pub fn language_id_for_extension(extension: &str) -> &'static str {
    match extension {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" | "ksh" => "shellscript",
        "fish" => "fish",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        "lua" => "lua",
        "zig" => "zig",
        "swift" => "swift",
        "vim" => "vim",
        "dockerfile" => "dockerfile",
        "make" | "mk" => "make",
        _ => "plaintext",
    }
}

/// Highlight one line, updating the multi-line scanner state.
pub fn highlight_line(line: &str, language: &str, state: &mut HighlightState) -> Vec<Span> {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut spans = Vec::new();
    let mut index = 0;

    while index < len {
        if state.in_block_comment {
            index = consume_block_comment(&chars, index, &mut spans, state);
            continue;
        }

        if starts_with(&chars, index, "/*") && supports_block_comments(language) {
            state.in_block_comment = true;
            index = consume_block_comment(&chars, index, &mut spans, state);
            continue;
        }

        if let Some(marker) = line_comment_marker(language) {
            if starts_with(&chars, index, marker) {
                spans.push(Span {
                    start: index,
                    end: len,
                    kind: TokenKind::Comment,
                });
                break;
            }
        }

        let ch = chars[index];

        if is_quote(ch, language) {
            let end = scan_string(&chars, index, ch);
            spans.push(Span {
                start: index,
                end,
                kind: TokenKind::String,
            });
            index = end;
            continue;
        }

        if ch.is_ascii_digit() {
            let end = scan_number(&chars, index);
            spans.push(Span {
                start: index,
                end,
                kind: TokenKind::Number,
            });
            index = end;
            continue;
        }

        if is_identifier_start(ch) {
            let end = scan_identifier(&chars, index);
            let word: String = chars[index..end].iter().collect();
            let kind = classify_word(&word, &chars, end, language);
            if let Some(kind) = kind {
                spans.push(Span {
                    start: index,
                    end,
                    kind,
                });
            }
            index = end;
            continue;
        }

        if is_operator(ch) {
            spans.push(Span {
                start: index,
                end: index + 1,
                kind: TokenKind::Operator,
            });
        }
        index += 1;
    }

    spans
}

fn consume_block_comment(
    chars: &[char],
    start: usize,
    spans: &mut Vec<Span>,
    state: &mut HighlightState,
) -> usize {
    let len = chars.len();
    let mut index = start + if state.in_block_comment { 0 } else { 2 };
    while index < len {
        if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
            spans.push(Span {
                start,
                end: index + 2,
                kind: TokenKind::Comment,
            });
            state.in_block_comment = false;
            return index + 2;
        }
        index += 1;
    }
    spans.push(Span {
        start,
        end: len,
        kind: TokenKind::Comment,
    });
    state.in_block_comment = true;
    len
}

fn supports_block_comments(language: &str) -> bool {
    !matches!(
        language,
        "python" | "yaml" | "toml" | "shellscript" | "fish" | "make" | "plaintext" | "ruby"
    )
}

fn line_comment_marker(language: &str) -> Option<&'static str> {
    match language {
        "rust" | "javascript" | "javascriptreact" | "typescript" | "typescriptreact" | "go"
        | "c" | "cpp" | "java" | "kotlin" | "swift" | "zig" | "json" => Some("//"),
        "python" | "toml" | "yaml" | "shellscript" | "fish" | "make" | "ruby" | "dockerfile" => {
            Some("#")
        }
        "sql" | "lua" => Some("--"),
        _ => None,
    }
}

fn is_quote(ch: char, language: &str) -> bool {
    ch == '"'
        || ch == '\''
        || (ch == '`'
            && matches!(
                language,
                "javascript" | "typescript" | "javascriptreact" | "typescriptreact" | "shellscript"
            ))
}

fn scan_string(chars: &[char], start: usize, quote: char) -> usize {
    let len = chars.len();
    let mut index = start + 1;
    while index < len {
        match chars[index] {
            '\\' => index += 2,
            ch if ch == quote => return index + 1,
            _ => index += 1,
        }
    }
    len
}

fn scan_number(chars: &[char], start: usize) -> usize {
    let len = chars.len();
    let mut index = start;
    while index < len {
        let ch = chars[index];
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | 'x' | 'X' | 'o' | 'b') {
            index += 1;
        } else {
            break;
        }
    }
    index
}

fn scan_identifier(chars: &[char], start: usize) -> usize {
    let len = chars.len();
    let mut index = start;
    while index < len && is_identifier_continue(chars[index]) {
        index += 1;
    }
    index
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '$'
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

fn is_operator(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '~' | '?' | ':'
    )
}

fn classify_word(word: &str, chars: &[char], end: usize, language: &str) -> Option<TokenKind> {
    if is_constant(word) {
        return Some(TokenKind::Constant);
    }
    if is_keyword(language, word) {
        return Some(TokenKind::Keyword);
    }
    if word.chars().next().is_some_and(|ch| ch.is_uppercase()) {
        return Some(TokenKind::Type);
    }
    let mut next = end;
    while next < chars.len() && chars[next].is_whitespace() {
        next += 1;
    }
    if chars.get(next) == Some(&'(') {
        return Some(TokenKind::Function);
    }
    None
}

fn is_constant(word: &str) -> bool {
    matches!(
        word,
        "true" | "false" | "null" | "nil" | "None" | "True" | "False" | "undefined"
    )
}

fn is_keyword(language: &str, word: &str) -> bool {
    let keywords: &[&str] = match language {
        "rust" => &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
            "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
            "type", "unsafe", "use", "where", "while", "yield",
        ],
        "python" => &[
            "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
            "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
            "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
            "with", "yield",
        ],
        "javascript" | "javascriptreact" | "typescript" | "typescriptreact" => &[
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "debugger",
            "default",
            "delete",
            "do",
            "else",
            "export",
            "extends",
            "finally",
            "for",
            "from",
            "function",
            "get",
            "if",
            "import",
            "in",
            "instanceof",
            "interface",
            "let",
            "new",
            "of",
            "private",
            "protected",
            "public",
            "return",
            "set",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "try",
            "type",
            "typeof",
            "var",
            "void",
            "while",
            "yield",
        ],
        "go" => &[
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "map",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "type",
            "var",
        ],
        "c" | "cpp" => &[
            "auto",
            "break",
            "case",
            "class",
            "const",
            "constexpr",
            "continue",
            "default",
            "delete",
            "do",
            "else",
            "enum",
            "explicit",
            "extern",
            "for",
            "if",
            "inline",
            "namespace",
            "new",
            "operator",
            "private",
            "protected",
            "public",
            "return",
            "sizeof",
            "static",
            "struct",
            "switch",
            "template",
            "this",
            "throw",
            "try",
            "typedef",
            "typename",
            "union",
            "using",
            "virtual",
            "void",
            "volatile",
            "while",
        ],
        "java" | "kotlin" => &[
            "abstract",
            "break",
            "case",
            "catch",
            "class",
            "continue",
            "data",
            "default",
            "do",
            "else",
            "enum",
            "extends",
            "final",
            "finally",
            "for",
            "fun",
            "if",
            "implements",
            "import",
            "interface",
            "object",
            "override",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "sealed",
            "static",
            "super",
            "switch",
            "this",
            "throw",
            "try",
            "val",
            "var",
            "void",
            "when",
            "while",
        ],
        "ruby" => &[
            "begin", "break", "case", "class", "def", "do", "else", "elsif", "end", "ensure",
            "for", "if", "in", "module", "next", "redo", "rescue", "retry", "return", "then",
            "unless", "until", "when", "while", "yield",
        ],
        "shellscript" | "fish" => &[
            "case", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
            "select", "then", "until", "while", "export", "local", "readonly", "return",
        ],
        "lua" => &[
            "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
            "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until",
            "while",
        ],
        "sql" => &[
            "alter", "and", "as", "by", "create", "delete", "drop", "from", "group", "having",
            "insert", "into", "join", "limit", "not", "null", "on", "or", "order", "select", "set",
            "table", "union", "update", "values", "where",
        ],
        "zig" => &[
            "align",
            "and",
            "asm",
            "break",
            "catch",
            "comptime",
            "const",
            "continue",
            "defer",
            "else",
            "enum",
            "errdefer",
            "error",
            "export",
            "extern",
            "fn",
            "for",
            "if",
            "inline",
            "or",
            "pub",
            "return",
            "struct",
            "switch",
            "test",
            "union",
            "unreachable",
            "usingnamespace",
            "var",
            "while",
        ],
        "swift" => &[
            "associatedtype",
            "class",
            "deinit",
            "enum",
            "extension",
            "func",
            "import",
            "init",
            "inout",
            "internal",
            "let",
            "open",
            "operator",
            "private",
            "protocol",
            "public",
            "static",
            "struct",
            "subscript",
            "typealias",
            "var",
        ],
        "markdown" => &[],
        _ => &[],
    };
    keywords.contains(&word)
}

fn starts_with(chars: &[char], index: usize, pattern: &str) -> bool {
    let mut remaining = chars[index.min(chars.len())..].iter();
    pattern
        .chars()
        .all(|expected| remaining.next() == Some(&expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_mapping_is_stable() {
        assert_eq!(language_id_for_path(Path::new("src/lib.rs")), "rust");
        assert_eq!(language_id_for_path(Path::new("Dockerfile")), "dockerfile");
        assert_eq!(language_id_for_path(Path::new("notes.txt")), "plaintext");
    }

    #[test]
    fn rust_keywords_and_strings_are_spanned() {
        let mut state = HighlightState::default();
        let spans = highlight_line("pub fn hello() { let s = \"hi\"; }", "rust", &mut state);
        assert!(spans.iter().any(|span| span.kind == TokenKind::Keyword));
        assert!(spans.iter().any(|span| span.kind == TokenKind::String));
        assert!(spans.iter().any(|span| span.kind == TokenKind::Function));
    }

    #[test]
    fn block_comments_carry_across_lines() {
        let mut state = HighlightState::default();
        let first = highlight_line("/* start", "rust", &mut state);
        assert_eq!(first.last().map(|span| span.kind), Some(TokenKind::Comment));
        assert!(state.in_block_comment);
        let second = highlight_line("end */ code", "rust", &mut state);
        assert_eq!(
            second.first().map(|span| span.kind),
            Some(TokenKind::Comment)
        );
        assert!(!state.in_block_comment);
    }

    #[test]
    fn hash_comments_apply_to_python() {
        let mut state = HighlightState::default();
        let spans = highlight_line("def f():  # comment", "python", &mut state);
        assert_eq!(spans.last().map(|span| span.kind), Some(TokenKind::Comment));
    }

    #[test]
    fn markdown_headings_are_not_comments() {
        let mut state = HighlightState::default();
        let spans = highlight_line("# Heading", "markdown", &mut state);
        assert!(spans.iter().all(|span| span.kind != TokenKind::Comment));
    }
}
