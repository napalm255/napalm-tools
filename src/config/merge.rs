//! Layering configuration sources into a single resolved view.
//!
//! Precedence, lowest to highest:
//!   catalog defaults -> `[bundles]` etc. -> matching `[host."..."]` tables in
//!   file order -> command-line flags.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

use super::file::{ConfigFile, Layer};
use super::hostmatch;
use crate::bundles::BUNDLES;
use crate::managers::ManagerId;

/// Dotfiles settings after resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotfilesConfig {
    /// Whether to manage dotfiles at all.
    pub enabled: bool,
    /// The chezmoi source repository.
    pub repo: Option<String>,
    /// Whether to run `chezmoi apply` on every run.
    pub apply: bool,
}

impl Default for DotfilesConfig {
    fn default() -> Self {
        DotfilesConfig {
            enabled: false,
            repo: None,
            apply: true,
        }
    }
}

/// Configuration after every layer has been applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Every known bundle, with its effective on/off state.
    pub bundles: BTreeMap<String, bool>,
    /// Extra packages beyond the catalog, by manager.
    pub extra: BTreeMap<ManagerId, Vec<String>>,
    /// Upgrade already-installed packages too.
    pub upgrade: bool,
    /// Fail when a package has no provider on this platform.
    pub strict: bool,
    /// Dotfiles bootstrap settings.
    pub dotfiles: DotfilesConfig,
}

impl Resolved {
    /// Whether the named bundle is enabled.
    pub fn bundle_enabled(&self, name: &str) -> bool {
        self.bundles.get(name).copied().unwrap_or(false)
    }
}

/// Overrides supplied on the command line. `None` means "not specified".
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// Bundle toggles from `--<bundle>` / `--no-<bundle>`.
    pub bundles: BTreeMap<String, bool>,
    /// `--upgrade`.
    pub upgrade: Option<bool>,
    /// `--strict`.
    pub strict: Option<bool>,
    /// `--no-dotfiles`.
    pub dotfiles_enabled: Option<bool>,
}

/// Resolve configuration for `hostname`.
pub fn resolve(file: &ConfigFile, hostname: &str, cli: &CliOverrides) -> Result<Resolved> {
    // 1. Catalog defaults.
    let mut resolved = Resolved {
        bundles: BUNDLES
            .iter()
            .map(|b| (b.name.to_string(), b.default_enabled))
            .collect(),
        extra: BTreeMap::new(),
        upgrade: false,
        strict: false,
        dotfiles: DotfilesConfig::default(),
    };

    // 2. Global settings.
    apply_layer(&mut resolved, &file.base).context("in the top-level configuration")?;

    // 3. Matching host overlays, in file order, so later entries win.
    for (pattern, value) in &file.host {
        if !hostmatch::matches(pattern, hostname)? {
            continue;
        }
        let layer: Layer = value
            .clone()
            .try_into()
            .with_context(|| format!("in [host.{pattern:?}]"))?;
        apply_layer(&mut resolved, &layer).with_context(|| format!("in [host.{pattern:?}]"))?;
    }

    // 4. Command line, which always wins.
    for (name, enabled) in &cli.bundles {
        set_bundle(&mut resolved, name, *enabled)?;
    }
    if let Some(v) = cli.upgrade {
        resolved.upgrade = v;
    }
    if let Some(v) = cli.strict {
        resolved.strict = v;
    }
    if let Some(v) = cli.dotfiles_enabled {
        resolved.dotfiles.enabled = v;
    }

    Ok(resolved)
}

/// Fold one layer's settings into the accumulating result.
fn apply_layer(resolved: &mut Resolved, layer: &Layer) -> Result<()> {
    for (name, enabled) in &layer.bundles {
        set_bundle(resolved, name, *enabled)?;
    }

    for (manager, packages) in &layer.extra {
        let id = manager_by_name(manager).with_context(|| {
            format!(
                "unknown package manager {manager:?} in [extra]; expected one of: {}",
                ManagerId::ALL
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        // Extras accumulate across layers rather than replacing, so a host
        // overlay adds to the global set instead of discarding it.
        let entry = resolved.extra.entry(id).or_default();
        for pkg in packages {
            if !entry.contains(pkg) {
                entry.push(pkg.clone());
            }
        }
    }

    if let Some(v) = layer.options.upgrade {
        resolved.upgrade = v;
    }
    if let Some(v) = layer.options.strict {
        resolved.strict = v;
    }
    if let Some(v) = layer.dotfiles.enabled {
        resolved.dotfiles.enabled = v;
    }
    if let Some(v) = &layer.dotfiles.repo {
        resolved.dotfiles.repo = Some(v.clone());
    }
    if let Some(v) = layer.dotfiles.apply {
        resolved.dotfiles.apply = v;
    }

    Ok(())
}

/// Set a bundle's state, rejecting names that are not in the catalog so a typo
/// fails loudly rather than silently doing nothing.
fn set_bundle(resolved: &mut Resolved, name: &str, enabled: bool) -> Result<()> {
    if !resolved.bundles.contains_key(name) {
        bail!(
            "unknown bundle {name:?}; known bundles: {}",
            BUNDLES
                .iter()
                .map(|b| b.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    resolved.bundles.insert(name.to_string(), enabled);
    Ok(())
}

/// Map a manager name from configuration onto a [`ManagerId`].
pub fn manager_by_name(name: &str) -> Option<ManagerId> {
    ManagerId::ALL.iter().copied().find(|m| m.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_text(text: &str, hostname: &str) -> Result<Resolved> {
        resolve(
            &ConfigFile::parse(text)?,
            hostname,
            &CliOverrides::default(),
        )
    }

    #[test]
    fn defaults_come_from_the_catalog() {
        let r = resolve_text("", "anyhost").unwrap();

        for b in BUNDLES {
            assert_eq!(
                r.bundle_enabled(b.name),
                b.default_enabled,
                "bundle {} should follow its catalog default",
                b.name
            );
        }
    }

    #[test]
    fn global_toggles_override_catalog_defaults() {
        let r = resolve_text("[bundles]\naws = true\ncore = false\n", "anyhost").unwrap();

        assert!(
            r.bundle_enabled("aws"),
            "aws defaults off, file turned it on"
        );
        assert!(
            !r.bundle_enabled("core"),
            "core defaults on, file turned it off"
        );
    }

    #[test]
    fn a_matching_host_table_overrides_globals() {
        let r = resolve_text(
            r#"
[bundles]
aws = false

[host."gibson"]
bundles = { aws = true }
"#,
            "gibson",
        )
        .unwrap();

        assert!(r.bundle_enabled("aws"));
    }

    #[test]
    fn a_non_matching_host_table_is_ignored() {
        let r = resolve_text(
            r#"
[bundles]
aws = false

[host."gibson"]
bundles = { aws = true }
"#,
            "napalm-desktop",
        )
        .unwrap();

        assert!(!r.bundle_enabled("aws"));
    }

    #[test]
    fn later_host_tables_win_over_earlier_ones() {
        // The general-to-specific ordering guarantee. Both patterns match.
        let r = resolve_text(
            r#"
[host."*"]
bundles = { aws = true }

[host."*.naponline.net"]
bundles = { aws = false }
"#,
            "napalm-desktop.local.naponline.net",
        )
        .unwrap();

        assert!(
            !r.bundle_enabled("aws"),
            "the later matching table must win"
        );
    }

    #[test]
    fn earlier_host_tables_do_not_win_when_order_is_reversed() {
        // Same two patterns, swapped. Proves the result tracks file order and
        // is not an artefact of pattern specificity.
        let r = resolve_text(
            r#"
[host."*.naponline.net"]
bundles = { aws = false }

[host."*"]
bundles = { aws = true }
"#,
            "napalm-desktop.local.naponline.net",
        )
        .unwrap();

        assert!(r.bundle_enabled("aws"), "the later matching table must win");
    }

    #[test]
    fn command_line_flags_beat_every_file_layer() {
        let file = ConfigFile::parse(
            r#"
[bundles]
desktop = true

[host."*"]
bundles = { desktop = true }
"#,
        )
        .unwrap();
        let cli = CliOverrides {
            bundles: BTreeMap::from([("desktop".to_string(), false)]),
            ..Default::default()
        };

        let r = resolve(&file, "anyhost", &cli).unwrap();

        assert!(!r.bundle_enabled("desktop"));
    }

    #[test]
    fn options_layer_from_file_then_cli() {
        let file = ConfigFile::parse("[options]\nupgrade = true\nstrict = true\n").unwrap();
        let cli = CliOverrides {
            strict: Some(false),
            ..Default::default()
        };

        let r = resolve(&file, "anyhost", &cli).unwrap();

        assert!(r.upgrade, "file value survives when the CLI is silent");
        assert!(!r.strict, "CLI value overrides the file");
    }

    #[test]
    fn options_default_to_off() {
        let r = resolve_text("", "anyhost").unwrap();

        assert!(!r.upgrade);
        assert!(!r.strict);
    }

    #[test]
    fn dotfiles_settings_layer_by_host() {
        let r = resolve_text(
            r#"
[dotfiles]
enabled = true
repo    = "https://github.com/napalm255/dotfiles"

[host."build-*"]
dotfiles = { enabled = false }
"#,
            "build-runner",
        )
        .unwrap();

        assert!(!r.dotfiles.enabled);
        assert_eq!(
            r.dotfiles.repo.as_deref(),
            Some("https://github.com/napalm255/dotfiles")
        );
        assert!(r.dotfiles.apply, "apply defaults on");
    }

    #[test]
    fn no_dotfiles_flag_overrides_the_file() {
        let file = ConfigFile::parse("[dotfiles]\nenabled = true\nrepo = \"x\"\n").unwrap();
        let cli = CliOverrides {
            dotfiles_enabled: Some(false),
            ..Default::default()
        };

        let r = resolve(&file, "anyhost", &cli).unwrap();

        assert!(!r.dotfiles.enabled);
    }

    #[test]
    fn extra_packages_are_keyed_by_manager() {
        let r = resolve_text("[extra]\nbrew = [\"jless\", \"dust\"]\n", "anyhost").unwrap();

        assert_eq!(
            r.extra.get(&ManagerId::Brew).unwrap(),
            &["jless".to_string(), "dust".to_string()]
        );
    }

    #[test]
    fn a_host_table_can_add_extra_packages() {
        let r = resolve_text(
            r#"
[extra]
brew = ["jless"]

[host."gibson"]
extra = { brew = ["dust"] }
"#,
            "gibson",
        )
        .unwrap();

        let brew = r.extra.get(&ManagerId::Brew).unwrap();
        assert!(
            brew.contains(&"jless".to_string()),
            "global extras are kept"
        );
        assert!(brew.contains(&"dust".to_string()), "host extras are added");
    }

    #[test]
    fn an_unknown_manager_in_extra_is_rejected() {
        let err = resolve_text("[extra]\npacman = [\"yay\"]\n", "anyhost").unwrap_err();

        assert!(
            format!("{err:#}").contains("pacman"),
            "error should name the offending manager, got: {err:#}"
        );
    }

    #[test]
    fn an_unknown_bundle_name_is_rejected() {
        // A typo'd bundle silently doing nothing is worse than a hard error.
        let err = resolve_text("[bundles]\ndevv = true\n", "anyhost").unwrap_err();

        assert!(
            format!("{err:#}").contains("devv"),
            "error should name the offending bundle, got: {err:#}"
        );
    }

    #[test]
    fn an_invalid_host_pattern_is_rejected() {
        let err = resolve_text("[host.\"[unclosed\"]\nbundles = {}\n", "anyhost").unwrap_err();

        assert!(format!("{err:#}").contains("[unclosed"), "got: {err:#}");
    }
}
