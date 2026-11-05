//! Configuration loading and deep-merge logic.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::Config;
use crate::platform::paths::PlatformPaths;

/// Configuration loading failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid configuration value: {0}")]
    Invalid(String),
}

/// Load the default configuration plus global and workspace overrides.
pub fn load(workspace_root: &Path) -> Result<Config, ConfigError> {
    let paths = PlatformPaths::discover();
    let global = paths.config_dir().join("config.toml");
    let workspace = workspace_config_path(workspace_root);
    load_from(Some(&global), Some(&workspace))
}

/// Path of the workspace-local configuration file.
pub fn workspace_config_path(root: &Path) -> PathBuf {
    root.join(".ttui").join("config.toml")
}

/// Load configuration from explicit global and workspace paths.
///
/// Missing files are treated as empty documents so a first run needs no
/// configuration at all.
pub fn load_from(global: Option<&Path>, workspace: Option<&Path>) -> Result<Config, ConfigError> {
    let mut merged = toml::Value::Table(toml::map::Map::new());

    for path in [global, workspace].into_iter().flatten() {
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        if text.trim().is_empty() {
            continue;
        }
        let value: toml::Value = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        deep_merge(&mut merged, value);
    }

    let mut config: Config = merged
        .try_into()
        .map_err(|error| ConfigError::Invalid(error.to_string()))?;
    config.normalize();
    Ok(config)
}

/// Recursively merge `overlay` into `base`, with tables merged key-by-key.
fn deep_merge(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base), toml::Value::Table(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => deep_merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn workspace_overrides_only_specified_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let global = dir.path().join("global.toml");
        let workspace = dir.path().join("workspace.toml");
        std::fs::write(
            &global,
            "[editor]\ntab_width = 8\n[ui]\ntheme = \"gruvbox\"\n",
        )
        .expect("write global");
        std::fs::write(&workspace, "[editor]\ntab_width = 2\n").expect("write workspace");

        let config = load_from(Some(&global), Some(&workspace)).expect("load");
        assert_eq!(config.editor.tab_width, 2);
        assert_eq!(config.ui.theme, "gruvbox");
        assert_eq!(config.ui.left_width, 28);
    }

    #[test]
    fn missing_files_fall_back_to_defaults() {
        let config = load_from(None, None).expect("load");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn invalid_toml_is_reported() {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        writeln!(file, "[editor\ntab_width = 2").expect("write");
        let error = load_from(Some(file.path()), None).expect_err("must fail");
        assert!(matches!(error, ConfigError::Parse { .. }));
    }
}
