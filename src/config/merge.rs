//! Layering configuration sources into a single resolved view.
//!
//! Precedence, lowest to highest:
//!   catalog (everything on) -> `[bundles]` etc. -> matching `[host."..."]`
//!   tables in file order -> command-line flags.
//!
//! This is also the boundary where user-supplied names are validated: a
//! bundle that does not exist, a manager nobody has heard of, a package name
//! that would be read as a flag. Nothing downstream re-checks them.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

use super::file::{ConfigFile, Layer};
use super::hostmatch;
use crate::bundles::{self, BUNDLES};
use crate::managers::ManagerId;

/// The prompts `[shell] prompt` may name, in the order they are offered.
pub const PROMPTS: &[&str] = &["starship", "oh-my-posh", "powerbash"];

/// The prompt used when nothing chooses one.
pub const DEFAULT_PROMPT: &str = "starship";

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
    /// The shell prompt to install and activate.
    pub prompt: String,
}

impl Resolved {
    /// Whether the named bundle is enabled.
    pub fn bundle_enabled(&self, name: &str) -> bool {
        self.bundles.get(name).copied().unwrap_or(false)
    }
}

/// Overrides supplied on the command line. `None` or empty means "not
/// specified".
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// `--skip <bundle>`: turn these off for this run.
    pub skip: Vec<String>,
    /// `--only <bundle>`: turn everything else off for this run.
    pub only: Vec<String>,
    /// `--upgrade`.
    pub upgrade: Option<bool>,
    /// `--strict`.
    pub strict: Option<bool>,
    /// `--no-dotfiles`.
    pub dotfiles_enabled: Option<bool>,
    /// `--prompt <name>`.
    pub prompt: Option<String>,
}

/// Resolve configuration for `hostname`.
pub fn resolve(file: &ConfigFile, hostname: &str, cli: &CliOverrides) -> Result<Resolved> {
    // 1. The catalog: everything on.
    let mut resolved = Resolved {
        bundles: BUNDLES.iter().map(|b| (b.name.to_string(), true)).collect(),
        extra: BTreeMap::new(),
        upgrade: false,
        strict: false,
        dotfiles: DotfilesConfig::default(),
        prompt: DEFAULT_PROMPT.to_string(),
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
    if !cli.only.is_empty() {
        for name in &cli.only {
            check_bundle(name)?;
        }
        for (name, on) in resolved.bundles.iter_mut() {
            *on = cli.only.iter().any(|o| o == name);
        }
    }
    for name in &cli.skip {
        set_bundle(&mut resolved, name, false)?;
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
    if let Some(p) = &cli.prompt {
        resolved.prompt = check_prompt(p)?;
    }

    if let Some(repo) = &resolved.dotfiles.repo {
        check_argument(repo).context("in [dotfiles] repo")?;
    }

    Ok(resolved)
}

/// Fold one layer's settings into the accumulating result.
fn apply_layer(resolved: &mut Resolved, layer: &Layer) -> Result<()> {
    for (name, enabled) in &layer.bundles {
        set_bundle(resolved, name, *enabled)?;
    }

    for (manager, packages) in &layer.extra {
        let id = ManagerId::from_name(manager).with_context(|| {
            format!(
                "unknown package manager {manager:?} in [extra]; expected one of: {}",
                ManagerId::names()
            )
        })?;
        // Extras accumulate across layers rather than replacing, so a host
        // overlay adds to the global set instead of discarding it.
        let entry = resolved.extra.entry(id).or_default();
        for pkg in packages {
            check_argument(pkg).with_context(|| format!("in [extra] {manager}"))?;
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
    if let Some(p) = &layer.shell.prompt {
        resolved.prompt = check_prompt(p).context("in [shell] prompt")?;
    }

    Ok(())
}

/// Set a bundle's state, rejecting names that are not in the catalog so a typo
/// fails loudly rather than silently doing nothing.
fn set_bundle(resolved: &mut Resolved, name: &str, enabled: bool) -> Result<()> {
    check_bundle(name)?;
    resolved.bundles.insert(name.to_string(), enabled);
    Ok(())
}

/// Reject a bundle name that is not in the catalog.
fn check_bundle(name: &str) -> Result<()> {
    if bundles::find(name).is_none() {
        bail!(
            "unknown bundle {name:?}; known bundles: {}",
            bundles::names()
        );
    }
    Ok(())
}

/// Reject a prompt that is not one of [`PROMPTS`].
fn check_prompt(name: &str) -> Result<String> {
    if !PROMPTS.contains(&name) {
        bail!(
            "unknown prompt {name:?}; expected one of: {}",
            PROMPTS.join(", ")
        );
    }
    Ok(name.to_string())
}

/// Reject a value that would not survive as a single command argument: empty,
/// containing whitespace, or shaped like a flag. Configuration is the user's
/// own, but `extra = ["--force"]` becoming `brew install --force` is a trap
/// worth closing at the boundary.
fn check_argument(value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("empty name");
    }
    if value.starts_with('-') {
        bail!("{value:?} looks like a flag, not a name");
    }
    if value.chars().any(char::is_whitespace) {
        bail!("{value:?} contains whitespace");
    }
    Ok(())
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
    fn every_bundle_is_on_by_default() {
        let r = resolve_text("", "anyhost").unwrap();

        for b in BUNDLES {
            assert!(r.bundle_enabled(b.name), "{} should default on", b.name);
        }
    }

    #[test]
    fn global_toggles_turn_a_bundle_off() {
        let r = resolve_text("[bundles]\nandroid = false\n", "anyhost").unwrap();

        assert!(!r.bundle_enabled("android"));
        assert!(r.bundle_enabled("core"), "others are untouched");
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
    fn skip_beats_every_file_layer() {
        let file = ConfigFile::parse("[host.\"*\"]\nbundles = { desktop = true }\n").unwrap();
        let cli = CliOverrides {
            skip: vec!["desktop".into()],
            ..Default::default()
        };

        let r = resolve(&file, "anyhost", &cli).unwrap();

        assert!(!r.bundle_enabled("desktop"));
    }

    #[test]
    fn only_turns_everything_else_off() {
        let cli = CliOverrides {
            only: vec!["core".into(), "rust".into()],
            ..Default::default()
        };

        let r = resolve(&ConfigFile::default(), "anyhost", &cli).unwrap();

        assert!(r.bundle_enabled("core"));
        assert!(r.bundle_enabled("rust"));
        assert!(!r.bundle_enabled("go"));
        assert!(!r.bundle_enabled("desktop"));
    }

    #[test]
    fn only_beats_a_file_that_turned_the_bundle_off() {
        // "--only x" means x, whatever the file said about x.
        let file = ConfigFile::parse("[bundles]\nrust = false\n").unwrap();
        let cli = CliOverrides {
            only: vec!["rust".into()],
            ..Default::default()
        };

        let r = resolve(&file, "anyhost", &cli).unwrap();

        assert!(r.bundle_enabled("rust"));
    }

    #[test]
    fn skip_applies_after_only() {
        let cli = CliOverrides {
            only: vec!["core".into(), "rust".into()],
            skip: vec!["rust".into()],
            ..Default::default()
        };

        let r = resolve(&ConfigFile::default(), "anyhost", &cli).unwrap();

        assert!(r.bundle_enabled("core"));
        assert!(!r.bundle_enabled("rust"));
    }

    #[test]
    fn an_unknown_bundle_on_the_command_line_is_rejected() {
        for cli in [
            CliOverrides {
                skip: vec!["nope".into()],
                ..Default::default()
            },
            CliOverrides {
                only: vec!["nope".into()],
                ..Default::default()
            },
        ] {
            let err = resolve(&ConfigFile::default(), "h", &cli).unwrap_err();
            assert!(format!("{err:#}").contains("nope"), "got {err:#}");
        }
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
    fn a_dotfiles_repo_shaped_like_a_flag_is_rejected() {
        let err = resolve_text("[dotfiles]\nrepo = \"--exec=evil\"\n", "h").unwrap_err();

        assert!(format!("{err:#}").contains("flag"), "got {err:#}");
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
    fn extras_may_name_mise_specs() {
        let r = resolve_text("[extra]\nmise = [\"terraform@latest\"]\n", "anyhost").unwrap();

        assert_eq!(
            r.extra.get(&ManagerId::Mise).unwrap(),
            &["terraform@latest".to_string()]
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
    fn an_extra_shaped_like_a_flag_is_rejected() {
        // `brew install --force` is not a package.
        let err = resolve_text("[extra]\nbrew = [\"--force\"]\n", "anyhost").unwrap_err();

        assert!(format!("{err:#}").contains("--force"), "got: {err:#}");
    }

    #[test]
    fn an_extra_with_whitespace_or_nothing_in_it_is_rejected() {
        assert!(resolve_text("[extra]\nbrew = [\"a b\"]\n", "h").is_err());
        assert!(resolve_text("[extra]\nbrew = [\"\"]\n", "h").is_err());
    }

    #[test]
    fn an_unknown_bundle_name_is_rejected() {
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

    #[test]
    fn the_prompt_defaults_to_starship() {
        assert_eq!(resolve_text("", "h").unwrap().prompt, "starship");
    }

    #[test]
    fn the_prompt_layers_like_everything_else() {
        let file = ConfigFile::parse(
            "[shell]\nprompt = \"powerbash\"\n[host.\"wsl-*\"]\nshell = { prompt = \"oh-my-posh\" }\n",
        )
        .unwrap();

        let r = resolve(&file, "wsl-box", &CliOverrides::default()).unwrap();
        assert_eq!(r.prompt, "oh-my-posh");

        let r = resolve(&file, "desk", &CliOverrides::default()).unwrap();
        assert_eq!(r.prompt, "powerbash");

        let cli = CliOverrides {
            prompt: Some("starship".into()),
            ..Default::default()
        };
        let r = resolve(&file, "desk", &cli).unwrap();
        assert_eq!(r.prompt, "starship", "the flag wins");
    }

    #[test]
    fn an_unknown_prompt_is_rejected() {
        let err = resolve_text("[shell]\nprompt = \"p10k\"\n", "h").unwrap_err();

        assert!(format!("{err:#}").contains("p10k"), "got: {err:#}");
        assert!(
            format!("{err:#}").contains("starship"),
            "should list choices"
        );
    }
}
