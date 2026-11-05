//! Permission request and decision modeling for ACP tool calls.

use serde_json::Value;

/// User decision for an agent permission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Cancelled,
}

impl PermissionDecision {
    /// Map a UI decision onto the option kind advertised by the agent.
    pub fn option_kind(self) -> Option<&'static str> {
        match self {
            Self::AllowOnce => Some("allow_once"),
            Self::AllowAlways => Some("allow_always"),
            Self::RejectOnce => Some("reject_once"),
            Self::RejectAlways => Some("reject_always"),
            Self::Cancelled => None,
        }
    }
}

/// Permission option kind as reported by the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Unknown(String),
}

impl PermissionOptionKind {
    pub fn parse_kind(value: &str) -> Self {
        match value {
            "allow_once" => Self::AllowOnce,
            "allow_always" => Self::AllowAlways,
            "reject_once" => Self::RejectOnce,
            "reject_always" => Self::RejectAlways,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::RejectOnce => "reject_once",
            Self::RejectAlways => "reject_always",
            Self::Unknown(other) => other,
        }
    }
}

/// One selectable option rendered in the permission card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

/// Pending `session/request_permission` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPrompt {
    /// JSON-RPC request id, required to respond to the agent.
    pub id: Value,
    pub session_id: String,
    pub tool_call: Value,
    pub options: Vec<PermissionOption>,
}

impl PermissionPrompt {
    /// Parse an ACP `session/request_permission` request.
    pub fn from_request(id: Value, params: &Value) -> Option<Self> {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        let option_id = option.get("optionId")?.as_str()?.to_string();
                        let name = option
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&option_id)
                            .to_string();
                        let kind = option
                            .get("kind")
                            .and_then(Value::as_str)
                            .map(PermissionOptionKind::parse_kind)
                            .unwrap_or_else(|| PermissionOptionKind::Unknown("unknown".into()));
                        Some(PermissionOption {
                            option_id,
                            name,
                            kind,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            id,
            session_id,
            tool_call,
            options,
        })
    }

    /// Resolve a decision to the agent-provided option id.
    pub fn select(&self, decision: PermissionDecision) -> Option<&PermissionOption> {
        let wanted = decision.option_kind()?;
        self.options
            .iter()
            .find(|option| option.kind.label() == wanted)
            .or_else(|| self.options.first())
    }

    /// One-line description of the tool call for the UI.
    pub fn summary(&self) -> String {
        self.tool_call
            .get("title")
            .or_else(|| self.tool_call.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("tool call")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_options_and_selects_kind() {
        let params = json!({
            "sessionId": "s1",
            "toolCall": { "title": "Edit src/main.rs" },
            "options": [
                { "optionId": "o1", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "o2", "name": "Reject", "kind": "reject_once" }
            ]
        });
        let prompt = PermissionPrompt::from_request(json!(7), &params).expect("parse");
        assert_eq!(prompt.summary(), "Edit src/main.rs");
        assert_eq!(
            prompt
                .select(PermissionDecision::AllowOnce)
                .map(|option| option.option_id.as_str()),
            Some("o1")
        );
        assert!(prompt.select(PermissionDecision::Cancelled).is_none());
    }
}
