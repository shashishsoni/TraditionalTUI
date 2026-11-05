//! Native and adapter-backed ACP agent discovery.

use std::collections::BTreeMap;

use crate::config::AgentSettings;
use crate::platform::shell::command_exists;

/// An installed or configured ACP agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentProfile {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub adapter: bool,
    pub available: bool,
    pub custom: bool,
}

impl AcpAgentProfile {
    fn builtin(
        id: &'static str,
        name: &'static str,
        command: &'static str,
        args: &[&'static str],
        adapter: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            command: command.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            adapter,
            available: command_exists(command),
            custom: false,
        }
    }

    /// Human-readable invocation for the AI panel.
    pub fn invocation(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

/// Built-in agents documented in `docs/ACP_INTEGRATION.md`.
pub fn builtin_profiles() -> Vec<AcpAgentProfile> {
    vec![
        AcpAgentProfile::builtin("opencode", "OpenCode", "opencode", &["acp"], false),
        AcpAgentProfile::builtin(
            "command-code",
            "Command Code",
            "command-code-acp",
            &[],
            false,
        ),
        AcpAgentProfile::builtin(
            "cursor-agent",
            "Cursor Agent",
            "cursor-agent",
            &["acp"],
            false,
        ),
        AcpAgentProfile::builtin("gemini", "Gemini CLI", "gemini", &["--acp"], false),
        AcpAgentProfile::builtin("goose", "Goose", "goose", &["acp"], false),
        AcpAgentProfile::builtin(
            "claude-agent",
            "Claude Agent (ACP adapter)",
            "claude-agent-acp",
            &[],
            true,
        ),
        AcpAgentProfile::builtin("codex", "Codex (ACP adapter)", "codex-acp", &[], true),
    ]
}

/// Merge built-in agents with workspace-configured custom agents.
pub fn discover(custom: &BTreeMap<String, AgentSettings>) -> Vec<AcpAgentProfile> {
    let mut profiles = builtin_profiles();
    for (id, settings) in custom {
        if settings.command.trim().is_empty() {
            continue;
        }
        profiles.push(AcpAgentProfile {
            id: id.clone(),
            name: id.clone(),
            command: settings.command.clone(),
            args: settings.args.clone(),
            adapter: settings.adapter,
            available: command_exists(&settings.command),
            custom: true,
        });
    }
    profiles
}

/// Look up an agent by id.
pub fn find<'a>(profiles: &'a [AcpAgentProfile], id: &str) -> Option<&'a AcpAgentProfile> {
    profiles.iter().find(|profile| profile.id == id)
}

/// Agents that can actually be launched right now.
pub fn available_profiles(profiles: &[AcpAgentProfile]) -> Vec<&AcpAgentProfile> {
    profiles
        .iter()
        .filter(|profile| profile.available)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_unique() {
        let profiles = builtin_profiles();
        let mut ids: Vec<_> = profiles.iter().map(|profile| &profile.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), profiles.len());
    }

    #[test]
    fn custom_agents_are_appended_only_when_configured() {
        let mut custom = BTreeMap::new();
        custom.insert(
            "my-agent".to_string(),
            AgentSettings {
                command: "definitely-not-installed-ttui".into(),
                ..Default::default()
            },
        );
        custom.insert("empty".to_string(), AgentSettings::default());
        let profiles = discover(&custom);
        assert!(profiles.iter().any(|profile| profile.id == "my-agent"));
        assert!(!profiles.iter().any(|profile| profile.id == "empty"));
        assert!(
            !profiles
                .iter()
                .find(|profile| profile.id == "my-agent")
                .expect("custom")
                .available
        );
    }

    #[test]
    fn disabled_agents_are_filtered_from_available_list() {
        let profiles = vec![AcpAgentProfile {
            id: "x".into(),
            name: "X".into(),
            command: "x".into(),
            args: Vec::new(),
            adapter: false,
            available: false,
            custom: true,
        }];
        assert!(available_profiles(&profiles).is_empty());
    }
}
