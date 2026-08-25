//! Serde representation of `config.toml`.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Settings that may appear either at the top level or inside a
/// `[host."..."]` table. Every field is optional so an overlay can adjust one
/// setting without restating the rest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Layer {
    /// Bundle toggles by name.
    pub bundles: BTreeMap<String, bool>,
    /// Extra packages by manager name — the escape hatch from the catalog.
    pub extra: BTreeMap<String, Vec<String>>,
    /// Run-behaviour options.
    pub options: OptionsLayer,
    /// Dotfiles bootstrap settings.
    pub dotfiles: DotfilesLayer,
}

/// Run-behaviour options, all optional at the layer level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OptionsLayer {
    /// Upgrade already-installed packages as well as installing missing ones.
    pub upgrade: Option<bool>,
    /// Exit non-zero when a package has no provider on this platform.
    pub strict: Option<bool>,
}

/// Dotfiles bootstrap settings, all optional at the layer level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DotfilesLayer {
    /// Whether to manage dotfiles at all.
    pub enabled: Option<bool>,
    /// The chezmoi source repository.
    pub repo: Option<String>,
    /// Whether to run `chezmoi apply` on every run.
    pub apply: Option<bool>,
}

/// A parsed `config.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    /// Settings that apply everywhere.
    #[serde(flatten)]
    pub base: Layer,
    /// Host-specific overlays, **keyed by glob and kept in file order**.
    /// Later entries win, so the file reads general to specific.
    pub host: toml::Table,
}

impl ConfigFile {
    /// Parse configuration from TOML text.
    pub fn parse(text: &str) -> anyhow::Result<ConfigFile> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_parses_to_defaults() {
        let c = ConfigFile::parse("").unwrap();

        assert!(c.base.bundles.is_empty());
        assert!(c.host.is_empty());
    }

    #[test]
    fn top_level_tables_populate_the_base_layer() {
        let c = ConfigFile::parse(
            r#"
[bundles]
core = true
aws  = false

[extra]
brew = ["jless", "dust"]

[options]
upgrade = true

[dotfiles]
enabled = true
repo    = "https://github.com/napalm255/dotfiles"
"#,
        )
        .unwrap();

        assert_eq!(c.base.bundles.get("core"), Some(&true));
        assert_eq!(c.base.bundles.get("aws"), Some(&false));
        assert_eq!(c.base.extra.get("brew").unwrap(), &["jless", "dust"]);
        assert_eq!(c.base.options.upgrade, Some(true));
        assert_eq!(c.base.dotfiles.enabled, Some(true));
    }

    #[test]
    fn host_tables_are_kept_in_file_order() {
        // The whole override scheme depends on this: later tables win, so the
        // parser must not sort or otherwise reorder them.
        let c = ConfigFile::parse(
            r#"
[host."*"]
bundles = { aws = true }

[host."gibson"]
bundles = { aws = false }

[host."*.example.com"]
bundles = { aws = true }
"#,
        )
        .unwrap();

        let keys: Vec<&str> = c.host.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["*", "gibson", "*.example.com"]);
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(ConfigFile::parse("[bundles\ncore = true").is_err());
    }
}
