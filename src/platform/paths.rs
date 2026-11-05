//! Standard directory resolution and path normalization.

use std::path::{Component, Path, PathBuf};

/// Resolved application directories following the documented XDG / macOS /
/// Windows conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
    state_dir: PathBuf,
    log_dir: PathBuf,
    home_dir: PathBuf,
}

impl PlatformPaths {
    /// Resolve paths from the environment, honoring `TTUI_*_DIR` overrides.
    pub fn discover() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_dir = env_path("TTUI_CONFIG_DIR")
            .or_else(|| dirs::config_dir().map(|path| path.join("ttui")))
            .unwrap_or_else(|| home_dir.join(".config").join("ttui"));
        let data_dir = env_path("TTUI_DATA_DIR")
            .or_else(|| dirs::data_dir().map(|path| path.join("ttui")))
            .unwrap_or_else(|| home_dir.join(".local").join("share").join("ttui"));
        let state_dir = env_path("TTUI_STATE_DIR")
            .or_else(|| dirs::data_local_dir().map(|path| path.join("ttui").join("state")))
            .unwrap_or_else(|| data_dir.join("state"));
        let log_dir = env_path("TTUI_LOG_DIR").unwrap_or_else(|| state_dir.join("logs"));

        Self {
            config_dir,
            data_dir,
            state_dir,
            log_dir,
            home_dir,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Ensure config, data, state and log directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.log_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Convert a path to a forward-slash string for display and fuzzy matching.
pub fn to_slash_lossy(path: &Path) -> String {
    let mut output = String::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => output.push_str(&prefix.as_os_str().to_string_lossy()),
            Component::RootDir => output.push('/'),
            Component::CurDir => {
                if !output.is_empty() {
                    output.push('/');
                }
                output.push('.');
            }
            Component::ParentDir => {
                if !output.is_empty() && !output.ends_with('/') {
                    output.push('/');
                }
                output.push_str("..");
            }
            Component::Normal(part) => {
                if !output.is_empty() && !output.ends_with('/') {
                    output.push('/');
                }
                output.push_str(&part.to_string_lossy());
            }
        }
    }
    output
}

/// Return `path` relative to `root` with `/` separators, falling back to the
/// full slash path when it is outside the root.
pub fn relative_display(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => to_slash_lossy(relative),
        Err(_) => to_slash_lossy(path),
    }
}

/// Canonicalize a path and remove the Windows `\\?\` verbatim prefix.
pub fn canonicalize_clean(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    Ok(strip_verbatim(canonical))
}

#[cfg(windows)]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        if rest.len() > 2 && rest.as_bytes().get(1) == Some(&b':') {
            return PathBuf::from(rest.to_string());
        }
    }
    path
}

#[cfg(not(windows))]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_normalization_is_stable() {
        let path = Path::new("/home/user/project/src/main.rs");
        assert_eq!(to_slash_lossy(path), "/home/user/project/src/main.rs");
    }

    #[test]
    fn relative_display_uses_forward_slashes() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/project/src/main.rs");
        assert_eq!(relative_display(root, file), "src/main.rs");
    }

    #[test]
    fn relative_display_falls_back_outside_root() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/etc/hosts");
        assert_eq!(relative_display(root, file), "/etc/hosts");
    }

    #[test]
    fn strip_verbatim_handles_windows_fixture() {
        let fixture = PathBuf::from(r"C:\Users\user\project\src\main.rs");
        assert_eq!(
            strip_verbatim(fixture.clone()),
            PathBuf::from(r"C:\Users\user\project\src\main.rs")
        );
    }

    #[test]
    fn discovered_paths_are_absolute() {
        let paths = PlatformPaths::discover();
        assert!(paths.config_dir().is_absolute());
        assert!(paths.state_dir().is_absolute());
    }
}
