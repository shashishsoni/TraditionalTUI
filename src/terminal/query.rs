//! Answers terminal capability queries emitted by interactive shells.
//!
//! Shells such as fish probe the terminal (background color, DA1, XTGETTCAP)
//! before drawing their prompt. Because `ttui` owns the PTY master, it must
//! reply on the shell's behalf; otherwise the shell blocks waiting forever and
//! the terminal panel appears blank.

const BG_COLOR: &[u8] = b"\x1b]11;rgb:1a1a/1b1b/2626\x1b\\";
const FG_COLOR: &[u8] = b"\x1b]10;rgb:c0c0/caca/f5f5\x1b\\";
const DA1_RESPONSE: &[u8] = b"\x1b[?62;22c";
const KITTY_KEYBOARD_UNSUPPORTED: &[u8] = b"\x1b[?0u";

/// Build responses for terminal queries found in `bytes`.
pub fn answers(bytes: &[u8], cursor: (u16, u16)) -> Vec<u8> {
    let mut response = Vec::new();

    if contains(bytes, b"\x1b]11;?") {
        response.extend_from_slice(BG_COLOR);
    }
    if contains(bytes, b"\x1b]10;?") {
        response.extend_from_slice(FG_COLOR);
    }
    if contains(bytes, b"\x1b[0c") || contains(bytes, b"\x1b[c") {
        response.extend_from_slice(DA1_RESPONSE);
    }
    if contains(bytes, b"\x1b[6n") {
        response.extend_from_slice(format!("\x1b[{};{}R", cursor.0 + 1, cursor.1 + 1).as_bytes());
    }
    if contains(bytes, b"\x1b[?u") {
        response.extend_from_slice(KITTY_KEYBOARD_UNSUPPORTED);
    }
    if contains(bytes, b"\x1b[>0q") {
        response.extend_from_slice(format!("\x1bP>|ttui {}\x1b\\", crate::VERSION).as_bytes());
    }

    // XTGETTCAP requests: DCS + q <hex-name>[=<hex-value>] ST.
    // Reply with a failure for each so the shell falls back to terminfo.
    let mut index = 0;
    while let Some(offset) = find(&bytes[index..], b"\x1bP+q") {
        let start = index + offset + 4;
        let Some(end_offset) = find(&bytes[start..], b"\x1b\\") else {
            break;
        };
        let name = &bytes[start..start + end_offset];
        let name = name.split(|byte| *byte == b'=').next().unwrap_or(name);
        response.extend_from_slice(b"\x1bP0+r");
        response.extend_from_slice(name);
        response.extend_from_slice(b"\x1b\\");
        index = start + end_offset + 2;
    }

    response
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_fish_startup_queries() {
        let mut probe = Vec::new();
        probe.extend_from_slice(b"\x1b]11;?\x1b\\");
        probe.extend_from_slice(b"\x1b[0c");
        probe.extend_from_slice(b"\x1bP+q696e646e\x1b\\");
        let response = answers(&probe, (2, 3));
        assert!(response.starts_with(b"\x1b]11"));
        assert!(response.windows(4).any(|window| window == b"\x1b[?6"));
        assert!(response.windows(7).any(|window| window == b"\x1bP0+r69"));
    }

    #[test]
    fn answers_cursor_position_report() {
        let response = answers(b"\x1b[6n", (5, 9));
        assert_eq!(response, b"\x1b[6;10R");
    }

    #[test]
    fn ignores_unrelated_output() {
        assert!(answers(b"hello world", (0, 0)).is_empty());
    }
}
