//! Preferences: loading, host-specific layering, and resolution.

pub mod file;
pub mod hostmatch;
pub mod merge;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use file::ConfigFile;
pub use merge::{CliOverrides, DotfilesConfig, Resolved, resolve};

/// Where the configuration file lives, given the relevant environment.
///
/// Pure, so the XDG rules can be tested without mutating process environment.
pub fn path_from_env(xdg_config_home: Option<&str>, home: Option<&str>) -> PathBuf {
    let base = match xdg_config_home {
        Some(x) if !x.is_empty() => PathBuf::from(x),
        _ => PathBuf::from(home.unwrap_or("")).join(".config"),
    };
    base.join("napalm-tools").join("config.toml")
}

/// The configuration path for this process.
///
/// `NT_CONFIG` overrides the XDG location, which keeps integration tests from
/// depending on the invoking user's real configuration.
pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("NT_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    path_from_env(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// Load configuration from `path`.
///
/// A missing file is not an error — it means "use the defaults". Anything else
/// (unreadable, malformed) is reported, because silently ignoring a config the
/// user wrote is worse than failing.
pub fn load(path: &Path) -> Result<ConfigFile> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            ConfigFile::parse(&text).with_context(|| format!("failed to parse {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_config_home_wins_when_set() {
        let p = path_from_env(Some("/custom/xdg"), Some("/home/napalm"));

        assert_eq!(p, PathBuf::from("/custom/xdg/napalm-tools/config.toml"));
    }

    #[test]
    fn falls_back_to_dot_config_under_home() {
        let p = path_from_env(None, Some("/home/napalm"));

        assert_eq!(
            p,
            PathBuf::from("/home/napalm/.config/napalm-tools/config.toml")
        );
    }

    #[test]
    fn an_empty_xdg_value_is_treated_as_unset() {
        let p = path_from_env(Some(""), Some("/home/napalm"));

        assert_eq!(
            p,
            PathBuf::from("/home/napalm/.config/napalm-tools/config.toml")
        );
    }

    #[test]
    fn a_missing_file_loads_as_defaults() {
        let c = load(Path::new("/nonexistent/napalm-tools/config.toml")).unwrap();

        assert!(c.base.bundles.is_empty());
    }

    #[test]
    fn a_malformed_file_is_an_error_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[bundles\ncore = true").unwrap();

        let err = load(&p).unwrap_err();

        assert!(
            format!("{err:#}").contains("config.toml"),
            "error should name the file, got: {err:#}"
        );
    }

    #[test]
    fn a_valid_file_is_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[bundles]\naws = true\n").unwrap();

        let c = load(&p).unwrap();

        assert_eq!(c.base.bundles.get("aws"), Some(&true));
    }
}
