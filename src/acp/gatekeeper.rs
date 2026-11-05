//! Workspace path confinement for ACP file and shell requests.

use std::path::{Path, PathBuf};

use super::AcpError;
use crate::platform::paths::canonicalize_clean;

/// Enforces that agent-requested paths stay inside the workspace root.
#[derive(Debug, Clone)]
pub struct Gatekeeper {
    root: PathBuf,
}

impl Gatekeeper {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root =
            canonicalize_clean(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validate an existing path for reading.
    pub fn resolve_existing(&self, requested: &Path) -> Result<PathBuf, AcpError> {
        if !requested.is_absolute() {
            return Err(AcpError::RelativePath(requested.to_path_buf()));
        }
        let canonical = canonicalize_clean(requested)?;
        if !canonical.starts_with(&self.root) {
            return Err(AcpError::OutsideWorkspace(requested.to_path_buf()));
        }
        Ok(canonical)
    }

    /// Validate a path for writing, allowing a not-yet-existing file.
    pub fn resolve_for_write(&self, requested: &Path) -> Result<PathBuf, AcpError> {
        if !requested.is_absolute() {
            return Err(AcpError::RelativePath(requested.to_path_buf()));
        }
        if requested.exists() {
            return self.resolve_existing(requested);
        }
        let parent = requested
            .parent()
            .ok_or_else(|| AcpError::OutsideWorkspace(requested.to_path_buf()))?;
        let parent = canonicalize_clean(parent)?;
        if !parent.starts_with(&self.root) {
            return Err(AcpError::OutsideWorkspace(requested.to_path_buf()));
        }
        let name = requested
            .file_name()
            .ok_or_else(|| AcpError::OutsideWorkspace(requested.to_path_buf()))?;
        Ok(parent.join(name))
    }

    /// Whether reading/writing this path warrants explicit confirmation.
    pub fn is_sensitive(&self, path: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name == ".env" || name.starts_with(".env.") || name == "id_rsa" || name == "id_ed25519" {
            return true;
        }
        let slash = crate::platform::paths::to_slash_lossy(path);
        slash.contains("/.git/config")
            || slash.contains("/.git/hooks/")
            || slash.contains("/.ssh/")
            || slash.contains("/.aws/")
            || slash.ends_with("/credentials")
    }

    /// Validate a read request, surfacing sensitive files separately.
    pub fn check_read(&self, requested: &Path) -> Result<PathBuf, AcpError> {
        let resolved = self.resolve_existing(requested)?;
        if self.is_sensitive(&resolved) {
            return Err(AcpError::SensitivePath(resolved));
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").expect("write");
        std::fs::write(dir.path().join(".env"), "SECRET=1").expect("write");
        dir
    }

    #[test]
    fn accepts_paths_inside_workspace() {
        let dir = workspace();
        let gate = Gatekeeper::new(dir.path());
        let resolved = gate
            .resolve_existing(&dir.path().join("src").join("main.rs"))
            .expect("inside");
        assert!(resolved.ends_with("main.rs"));
    }

    #[test]
    fn rejects_traversal_and_absolute_escape() {
        let dir = workspace();
        let gate = Gatekeeper::new(dir.path());
        assert!(matches!(
            gate.resolve_existing(Path::new("/etc/passwd")),
            Err(AcpError::OutsideWorkspace(_))
        ));
        assert!(matches!(
            gate.resolve_existing(Path::new("../../etc/passwd")),
            Err(AcpError::RelativePath(_))
        ));
    }

    #[test]
    fn write_resolution_allows_new_files_inside_root() {
        let dir = workspace();
        let gate = Gatekeeper::new(dir.path());
        let target = gate
            .resolve_for_write(&dir.path().join("src").join("new.rs"))
            .expect("new file");
        assert_eq!(target.file_name().and_then(|n| n.to_str()), Some("new.rs"));
    }

    #[test]
    fn sensitive_dotfiles_require_confirmation() {
        let dir = workspace();
        let gate = Gatekeeper::new(dir.path());
        assert!(matches!(
            gate.check_read(&dir.path().join(".env")),
            Err(AcpError::SensitivePath(_))
        ));
    }

    #[test]
    fn write_outside_root_is_rejected() {
        let dir = workspace();
        let gate = Gatekeeper::new(dir.path());
        assert!(matches!(
            gate.resolve_for_write(&dir.path().join("..").join("escape.rs")),
            Err(AcpError::OutsideWorkspace(_))
        ));
    }
}
