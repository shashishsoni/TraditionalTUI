//! Structured line diffs for AI/ACP edit previews.

use std::path::PathBuf;

use imara_diff::intern::InternedInput;
use imara_diff::sources::lines;
use imara_diff::{Algorithm, diff};
use serde_json::Value;

/// Classification of one preview line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Removed,
    Added,
}

/// One line in a file edit preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

/// A file edit proposed by an agent or provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPreview {
    pub path: PathBuf,
    pub lines: Vec<DiffLine>,
}

impl DiffPreview {
    /// Build a full line diff for a before/after file pair.
    pub fn compute(path: PathBuf, before: &str, after: &str) -> Self {
        let before_lines: Vec<&str> = before.lines().collect();
        let after_lines: Vec<&str> = after.lines().collect();
        let input = InternedInput::new(lines(before), lines(after));
        let mut output = Vec::new();
        let mut old_pos = 0usize;
        let mut new_pos = 0usize;

        diff(
            Algorithm::Histogram,
            &input,
            |before_range: std::ops::Range<u32>, after_range: std::ops::Range<u32>| {
                let old_start = before_range.start as usize;
                let old_end = before_range.end as usize;
                let new_start = after_range.start as usize;
                let new_end = after_range.end as usize;

                while old_pos < old_start && new_pos < new_start {
                    output.push(DiffLine {
                        kind: DiffLineKind::Context,
                        text: before_lines.get(old_pos).copied().unwrap_or("").to_string(),
                        old_line: Some(old_pos as u32 + 1),
                        new_line: Some(new_pos as u32 + 1),
                    });
                    old_pos += 1;
                    new_pos += 1;
                }
                for index in old_start..old_end {
                    output.push(DiffLine {
                        kind: DiffLineKind::Removed,
                        text: before_lines.get(index).copied().unwrap_or("").to_string(),
                        old_line: Some(index as u32 + 1),
                        new_line: None,
                    });
                }
                for index in new_start..new_end {
                    output.push(DiffLine {
                        kind: DiffLineKind::Added,
                        text: after_lines.get(index).copied().unwrap_or("").to_string(),
                        old_line: None,
                        new_line: Some(index as u32 + 1),
                    });
                }
                old_pos = old_end;
                new_pos = new_end;
            },
        );

        while old_pos < before_lines.len() {
            output.push(DiffLine {
                kind: DiffLineKind::Context,
                text: before_lines[old_pos].to_string(),
                old_line: Some(old_pos as u32 + 1),
                new_line: (new_pos < after_lines.len()).then_some(new_pos as u32 + 1),
            });
            old_pos += 1;
            new_pos += 1;
        }

        Self {
            path,
            lines: output,
        }
    }

    /// Extract the first `ContentBlock::Diff` from an ACP tool call or update.
    pub fn from_tool_call(value: &Value) -> Option<Self> {
        find_diff(value, 0)
    }

    pub fn added(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Added)
            .count()
    }

    pub fn removed(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Removed)
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn find_diff(value: &Value, depth: usize) -> Option<DiffPreview> {
    if depth > 10 {
        return None;
    }
    if value.get("type").and_then(Value::as_str) == Some("diff") {
        let path = value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let old_text = value
            .get("oldText")
            .or_else(|| value.get("old_text"))
            .and_then(Value::as_str);
        let new_text = value
            .get("newText")
            .or_else(|| value.get("new_text"))
            .and_then(Value::as_str);
        if let (Some(old_text), Some(new_text)) = (old_text, new_text) {
            return Some(DiffPreview::compute(
                PathBuf::from(path),
                old_text,
                new_text,
            ));
        }
    }

    match value {
        Value::Array(items) => items.iter().find_map(|item| find_diff(item, depth + 1)),
        Value::Object(map) => map.values().find_map(|item| find_diff(item, depth + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn computes_removed_and_added_lines() {
        let preview = DiffPreview::compute(
            PathBuf::from("src/main.rs"),
            "fn main() {\n    old();\n}\n",
            "fn main() {\n    new();\n    more();\n}\n",
        );
        assert_eq!(preview.removed(), 1);
        assert_eq!(preview.added(), 2);
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Removed && line.text.contains("old"))
        );
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.kind == DiffLineKind::Added && line.text.contains("new"))
        );
    }

    #[test]
    fn extracts_diff_from_tool_call() {
        let value = json!({
            "toolCall": {
                "content": [{
                    "type": "diff",
                    "path": "/tmp/a.rs",
                    "oldText": "one\n",
                    "newText": "two\n"
                }]
            }
        });
        let preview = DiffPreview::from_tool_call(&value).expect("diff");
        assert_eq!(preview.path, PathBuf::from("/tmp/a.rs"));
        assert_eq!(preview.removed(), 1);
        assert_eq!(preview.added(), 1);
    }

    #[test]
    fn ignores_non_diff_payloads() {
        assert!(DiffPreview::from_tool_call(&json!({"content": [{"type": "text"}]})).is_none());
    }
}
