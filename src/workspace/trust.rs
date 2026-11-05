//! Workspace trust store.
//!
//! Executable workspace configuration (custom LSP/ACP commands) is only
//! honored after the user explicitly trusts the workspace. The trust entry
//! binds the canonical root to a fingerprint of `.ttui/config.toml`; editing
//! executable settings invalidates the fingerprint and re-prompts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Persistent store of trusted workspace roots.
#[derive(Debug, Clone)]
pub struct TrustStore {
    path: PathBuf,
    entries: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    workspaces: BTreeMap<String, String>,
}

impl TrustStore {
    /// Load a trust store from `path`, falling back to an empty store.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoreFile>(&bytes).ok())
            .map(|file| file.workspaces)
            .unwrap_or_default();
        Self { path, entries }
    }

    fn key(root: &Path) -> String {
        crate::platform::paths::to_slash_lossy(root)
    }

    /// Stable fingerprint of executable workspace settings.
    pub fn fingerprint(root: &Path) -> String {
        let config = root.join(".ttui").join("config.toml");
        let bytes = std::fs::read(config).unwrap_or_default();
        format!("{:016x}", fnv1a(&bytes))
    }

    /// Whether `root` is trusted and its configuration is unchanged.
    pub fn is_trusted(&self, root: &Path) -> bool {
        let key = Self::key(root);
        self.entries
            .get(&key)
            .is_some_and(|fingerprint| fingerprint == &Self::fingerprint(root))
    }

    /// Trust `root` with its current configuration fingerprint.
    pub fn trust(&mut self, root: &Path) -> std::io::Result<()> {
        self.entries
            .insert(Self::key(root), Self::fingerprint(root));
        self.persist()
    }

    /// Revoke trust for `root`.
    pub fn revoke(&mut self, root: &Path) -> std::io::Result<()> {
        self.entries.remove(&Self::key(root));
        self.persist()
    }

    fn persist(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = StoreFile {
            workspaces: self.entries.clone(),
        };
        let mut bytes = serde_json::to_vec_pretty(&file).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        crate::platform::fs::atomic_write(&self.path, &bytes)
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_bound_to_config_fingerprint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join(".ttui")).expect("mkdir");
        let store_path = dir.path().join("trust.json");

        let mut store = TrustStore::load(&store_path);
        assert!(!store.is_trusted(&root));
        store.trust(&root).expect("trust");
        assert!(store.is_trusted(&root));

        std::fs::write(
            root.join(".ttui").join("config.toml"),
            "[acp]\nenabled = true\n",
        )
        .expect("write");
        assert!(
            !store.is_trusted(&root),
            "config change must invalidate trust"
        );
    }

    #[test]
    fn fingerprint_is_stable() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            TrustStore::fingerprint(dir.path()),
            TrustStore::fingerprint(dir.path())
        );
    }
}
