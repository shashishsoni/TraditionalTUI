//! Shell detection for integrated terminals and task runners.

use std::path::{Path, PathBuf};

/// Detect the user's login shell.
///
/// Resolution: explicit override -> platform-specific probe -> sensible
/// fallback (`/bin/sh` on Unix, `cmd.exe` on Windows).
pub fn detect_shell(override_command: Option<&str>) -> PathBuf {
    if let Some(command) = override_command {
        if !command.trim().is_empty() {
            return PathBuf::from(command);
        }
    }
    platform_default_shell()
}

#[cfg(unix)]
fn platform_default_shell() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL") {
        let path = PathBuf::from(shell);
        if path.is_file() {
            return path;
        }
    }
    for candidate in ["/bin/zsh", "/bin/bash", "/bin/fish", "/bin/sh"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from("/bin/sh")
}

#[cfg(windows)]
fn platform_default_shell() -> PathBuf {
    for candidate in ["pwsh.exe", "powershell.exe"] {
        if let Some(path) = find_in_path(candidate) {
            return path;
        }
    }
    if let Some(comspec) = std::env::var_os("COMSPEC") {
        return PathBuf::from(comspec);
    }
    PathBuf::from("cmd.exe")
}

#[cfg(not(any(unix, windows)))]
fn platform_default_shell() -> PathBuf {
    PathBuf::from("/bin/sh")
}

/// Locate an executable on `PATH`.
pub fn find_in_path(executable: &str) -> Option<PathBuf> {
    if executable.contains(std::path::MAIN_SEPARATOR) || executable.contains('/') {
        let path = Path::new(executable);
        return path.is_file().then(|| path.to_path_buf());
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Whether `executable` resolves on this machine.
pub fn command_exists(executable: &str) -> bool {
    find_in_path(executable).is_some()
}

/// Last-resort shell used when the preferred shell cannot be spawned.
pub fn fallback_shell() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(comspec) = std::env::var_os("COMSPEC") {
            return PathBuf::from(comspec);
        }
        PathBuf::from("cmd.exe")
    }
    #[cfg(not(windows))]
    {
        for candidate in ["/bin/sh", "/usr/bin/sh"] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return path;
            }
        }
        PathBuf::from("/bin/sh")
    }
}

/// Display name of a shell path (e.g. `/bin/zsh` -> `zsh`).
pub fn shell_display_name(shell: &Path) -> String {
    shell
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("shell")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        assert_eq!(detect_shell(Some("/bin/sh")), PathBuf::from("/bin/sh"));
    }

    #[test]
    fn shell_name_is_stripped() {
        assert_eq!(shell_display_name(Path::new("/bin/zsh")), "zsh");
    }

    #[test]
    fn missing_binary_is_not_found() {
        assert!(!command_exists("definitely-not-a-real-binary-ttui"));
    }
}
