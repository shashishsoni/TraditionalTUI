//! Context assembly with a bounded token budget.
//!
//! Sections are included in priority order until the model budget is
//! exhausted. The estimate is intentionally conservative (`chars / 4`).

/// Hard limits for assembled context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLimits {
    /// Maximum characters sent to the model in one turn.
    pub max_chars: usize,
    /// Fraction of the model context window that may be used (percent).
    pub window_percent: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        Self {
            max_chars: 48_000,
            window_percent: 50,
        }
    }
}

/// A labeled context section used for assembly and UI inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSection {
    pub title: String,
    pub content: String,
}

/// Assembled prompt payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextBundle {
    pub system: String,
    pub user: String,
    pub sections: Vec<ContextSection>,
    pub estimated_tokens: usize,
    pub truncated: bool,
}

impl ContextBundle {
    pub fn is_empty(&self) -> bool {
        self.user.is_empty() && self.system.is_empty()
    }
}

/// Builds a priority-ordered bundle from editor state.
#[derive(Debug, Clone, Default)]
pub struct ContextAssembler {
    limits: ContextLimits,
}

impl ContextAssembler {
    pub fn new(limits: ContextLimits) -> Self {
        Self { limits }
    }

    pub fn limits(&self) -> ContextLimits {
        self.limits
    }

    /// Assemble context for `instruction`.
    ///
    /// Priority: instruction -> diagnostics -> selection -> surrounding code ->
    /// explicit `@` mentions.
    pub fn assemble(
        &self,
        instruction: &str,
        selection: Option<&str>,
        surrounding: Option<&str>,
        diagnostics: &[String],
        mentions: &[String],
        context_window: usize,
    ) -> ContextBundle {
        let window_chars = context_window
            .saturating_mul(self.limits.window_percent)
            .saturating_div(100)
            .saturating_mul(4);
        let budget = self.limits.max_chars.min(window_chars.max(1_000));

        let mut bundle = ContextBundle::default();
        let mut used = 0usize;
        let push = |title: &str, content: &str, bundle: &mut ContextBundle, used: &mut usize| {
            if content.trim().is_empty() || *used >= budget {
                if !content.trim().is_empty() && *used >= budget {
                    bundle.truncated = true;
                }
                return;
            }
            let remaining = budget - *used;
            let (content, cut) = if content.len() > remaining {
                (split_at_char_boundary(content, remaining), true)
            } else {
                (content, false)
            };
            if cut {
                bundle.truncated = true;
            }
            bundle.sections.push(ContextSection {
                title: title.to_string(),
                content: content.to_string(),
            });
            *used += content.len();
        };

        push("Instruction", instruction, &mut bundle, &mut used);

        if !diagnostics.is_empty() {
            let joined = diagnostics.join("\n");
            push("Diagnostics", &joined, &mut bundle, &mut used);
        }
        if let Some(selection) = selection {
            push("Selection", selection, &mut bundle, &mut used);
        }
        if let Some(surrounding) = surrounding {
            push("Surrounding code", surrounding, &mut bundle, &mut used);
        }
        for mention in mentions {
            push("Mention", mention, &mut bundle, &mut used);
        }

        let mut system = String::from(
            "You are the AI assistant embedded in TradinationalTUI, a terminal IDE. \
             Be concise, prefer code, and never invent file contents.",
        );
        if bundle.truncated {
            system.push_str("\nSome context was truncated to fit the model window.");
        }
        bundle.system = system;
        bundle.user = bundle
            .sections
            .iter()
            .map(|section| format!("## {}\n{}", section.title, section.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        bundle.estimated_tokens = bundle.user.chars().count().div_ceil(4);
        bundle
    }
}

fn split_at_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_priority_sections() {
        let assembler = ContextAssembler::default();
        let bundle = assembler.assemble(
            "explain this",
            Some("let x = 1;"),
            Some("fn main() {}"),
            &["unused variable".to_string()],
            &["src/main.rs".to_string()],
            100_000,
        );
        let titles: Vec<_> = bundle
            .sections
            .iter()
            .map(|section| section.title.as_str())
            .collect();
        assert_eq!(
            titles,
            vec![
                "Instruction",
                "Diagnostics",
                "Selection",
                "Surrounding code",
                "Mention"
            ]
        );
        assert!(bundle.estimated_tokens > 0);
        assert!(!bundle.truncated);
    }

    #[test]
    fn truncates_when_budget_is_tiny() {
        let assembler = ContextAssembler::new(ContextLimits {
            max_chars: 20,
            window_percent: 100,
        });
        let bundle = assembler.assemble(
            "a very long instruction that cannot fit",
            Some("selection"),
            None,
            &[],
            &[],
            10_000,
        );
        assert!(bundle.truncated);
        assert!(bundle.user.len() <= 40);
    }

    #[test]
    fn respects_small_context_window() {
        let assembler = ContextAssembler::default();
        let bundle =
            assembler.assemble("do work", Some(&"x".repeat(10_000)), None, &[], &[], 1_000);
        assert!(bundle.truncated);
        assert!(bundle.user.len() < 10_000);
    }
}
