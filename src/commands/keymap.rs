//! Keymap chord resolution.

use std::collections::BTreeMap;

use crate::commands::ids;
use crate::config::KeymapPreset;

/// Parsed keyboard chord, e.g. `ctrl-shift-p` or `alt-1`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Chord {
    /// Parse a chord string. Modifiers may appear in any order and with `+`
    /// or `-` separators. The final token is the key.
    pub fn parse(text: &str) -> Option<Self> {
        let normalized = text.trim().replace('+', "-");
        let mut tokens: Vec<String> = normalized
            .split('-')
            .filter(|token| !token.is_empty())
            .map(|token| token.to_ascii_lowercase())
            .collect();
        if tokens.is_empty() {
            return None;
        }
        let key = tokens.pop()?;
        let mut chord = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            key,
        };
        for token in tokens {
            match token.as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "alt" | "option" | "meta" => chord.alt = true,
                "shift" => chord.shift = true,
                _ => return None,
            }
        }
        Some(chord)
    }

    /// Canonical display/key form.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        parts.join("-")
    }
}

/// Keyboard mapping from chords to command ids.
#[derive(Debug, Clone)]
pub struct Keymap {
    preset: KeymapPreset,
    bindings: BTreeMap<String, String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::for_preset(KeymapPreset::Vim)
    }
}

impl Keymap {
    pub fn for_preset(preset: KeymapPreset) -> Self {
        let mut bindings = BTreeMap::new();
        let defaults: &[(&str, &str)] = &[
            ("ctrl-s", ids::SAVE),
            ("ctrl-q", ids::QUIT),
            ("ctrl-b", ids::TOGGLE_EXPLORER),
            ("ctrl-j", ids::TOGGLE_BOTTOM),
            ("ctrl-alt-b", ids::TOGGLE_AI),
            ("ctrl-p", ids::FILE_FINDER),
            ("ctrl-shift-p", ids::COMMAND_PALETTE),
            ("ctrl-shift-f", ids::PROJECT_SEARCH),
            ("ctrl-f", ids::BUFFER_SEARCH),
            ("ctrl-z", ids::UNDO),
            ("ctrl-y", ids::REDO),
            ("ctrl-a", ids::SELECT_ALL),
            ("ctrl-c", ids::COPY),
            ("ctrl-v", ids::PASTE),
            ("ctrl-shift-t", ids::NEW_TERMINAL),
            ("f1", ids::HELP),
            ("f2", ids::CONNECT_AGENT),
            ("f3", ids::NEW_TERMINAL),
            ("f4", ids::OPEN_CONFIG),
            ("ctrl-shift-a", ids::CONNECT_AGENT),
            ("ctrl-,", ids::OPEN_CONFIG),
            ("ctrl-space", ids::COMMAND_PALETTE),
            ("ctrl-t", ids::NEW_TERMINAL),
            ("ctrl-g", ids::CONNECT_AGENT),
            ("ctrl-o", ids::OPEN_CONFIG),
            ("alt-t", ids::NEW_TERMINAL),
            ("alt-a", ids::CONNECT_AGENT),
            ("alt-c", ids::OPEN_CONFIG),
            ("alt-p", ids::COMMAND_PALETTE),
            ("alt-o", ids::OPEN_FOLDER),
            ("alt-b", ids::BRANCH_PICKER),
            ("ctrl-alt-o", ids::OPEN_FOLDER),
            ("ctrl-shift-e", ids::SHOW_EXPLORER),
            ("ctrl-shift-f", ids::PROJECT_SEARCH),
            ("alt-e", ids::SHOW_EXPLORER),
            ("alt-g", ids::SHOW_SOURCE_CONTROL),
            ("alt-f", ids::PROJECT_SEARCH),
            ("alt-d", ids::SHOW_DIFF),
            ("f5", ids::SHOW_EXPLORER),
            ("f6", ids::SHOW_SOURCE_CONTROL),
            ("f7", ids::PROJECT_SEARCH),
            ("f8", ids::SHOW_DIFF),
        ];
        for (chord, command) in defaults {
            if let Some(chord) = Chord::parse(chord) {
                bindings.insert(chord.canonical(), (*command).to_string());
            }
        }
        match preset {
            KeymapPreset::Vim => {
                bindings.insert("escape".into(), ids::NORMAL_MODE.into());
            }
            KeymapPreset::Vscode => {
                bindings.insert("escape".into(), ids::NORMAL_MODE.into());
            }
        }
        Self { preset, bindings }
    }

    pub fn preset(&self) -> KeymapPreset {
        self.preset
    }

    /// Resolve a chord string to a command id.
    pub fn resolve(&self, chord: &str) -> Option<&str> {
        let chord = Chord::parse(chord)?;
        self.bindings.get(&chord.canonical()).map(String::as_str)
    }

    /// First binding registered for a command id.
    pub fn binding_for(&self, command_id: &str) -> Option<&str> {
        self.bindings
            .iter()
            .find(|(_, command)| command.as_str() == command_id)
            .map(|(chord, _)| chord.as_str())
    }

    pub fn bind(&mut self, chord: &str, command_id: &str) -> bool {
        let Some(chord) = Chord::parse(chord) else {
            return false;
        };
        self.bindings
            .insert(chord.canonical(), command_id.to_string());
        true
    }

    pub fn bindings(&self) -> &BTreeMap<String, String> {
        &self.bindings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_parse_normalizes_order() {
        assert_eq!(
            Chord::parse("Shift+Ctrl+P").map(|chord| chord.canonical()),
            Some("ctrl-shift-p".to_string())
        );
        assert_eq!(
            Chord::parse("alt-1").map(|chord| chord.canonical()),
            Some("alt-1".to_string())
        );
    }

    #[test]
    fn unknown_modifier_is_rejected() {
        assert!(Chord::parse("hyper-x").is_none());
    }

    #[test]
    fn keymap_resolves_ctrl_shortcuts() {
        let keymap = Keymap::for_preset(KeymapPreset::Vim);
        assert_eq!(keymap.resolve("ctrl-s"), Some(ids::SAVE));
        assert_eq!(keymap.resolve("ctrl-shift-p"), Some(ids::COMMAND_PALETTE));
        assert_eq!(keymap.resolve("nonexistent-key"), None);
    }

    #[test]
    fn binding_lookup_is_reverse_mapped() {
        let keymap = Keymap::for_preset(KeymapPreset::Vscode);
        assert_eq!(keymap.binding_for(ids::QUIT), Some("ctrl-q"));
    }
}
