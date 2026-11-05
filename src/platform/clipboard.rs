//! Clipboard integration with OSC 52 terminal fallback.
//!
//! Native desktop clipboard integration is intentionally represented by the
//! OSC 52 escape sequence: it works over SSH, inside tmux (with passthrough)
//! and in every modern terminal without linking to a GUI toolkit.

/// Clipboard operation backend chosen for the current session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    /// Plain OSC 52 sequence understood by the host terminal.
    Osc52,
    /// OSC 52 wrapped in a tmux passthrough sequence.
    TmuxOsc52,
}

/// Clipboard helper.
#[derive(Debug, Clone, Copy, Default)]
pub struct Clipboard;

impl Clipboard {
    /// Choose the sequence based on the environment.
    pub fn detect_mode() -> ClipboardMode {
        if std::env::var_os("TMUX").is_some() {
            ClipboardMode::TmuxOsc52
        } else {
            ClipboardMode::Osc52
        }
    }

    /// Build the escape sequence that copies `text` to the host clipboard.
    pub fn copy_sequence(text: &str, mode: ClipboardMode) -> String {
        let encoded = base64_encode(text.as_bytes());
        match mode {
            ClipboardMode::Osc52 => format!("\x1b]52;c;{encoded}\x07"),
            ClipboardMode::TmuxOsc52 => format!("\x1bPtmux;\x1b\x1b]52;c;{encoded}\x07\x1b\\"),
        }
    }

    /// Convenience: detect mode and build the copy sequence.
    pub fn copy(text: &str) -> String {
        Self::copy_sequence(text, Self::detect_mode())
    }
}

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        output.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn osc52_sequence_is_wrapped() {
        let sequence = Clipboard::copy_sequence("hi", ClipboardMode::Osc52);
        assert_eq!(sequence, "\x1b]52;c;aGk=\x07");
    }
}
