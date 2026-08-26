//! chezmoi bootstrap.
//!
//! Runs after packages have converged, so `chezmoi` itself can come from the
//! catalog on a fresh machine.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use crate::config::DotfilesConfig;
use crate::managers::Cmd;

/// Where chezmoi keeps its source state.
pub fn source_dir(home: &Path) -> PathBuf {
    home.join(".local").join("share").join("chezmoi")
}

/// Decide what to run for the dotfiles step.
///
/// Returns an empty list when there is nothing to do, so the caller does not
/// need to special-case a disabled configuration.
///
/// `may_need_privileges` marks the step so the run asks for a password up
/// front. chezmoi runs the user's own `run_` scripts, which may legitimately
/// use sudo, and a prompt arriving mid-run lands underneath the spinner where
/// it cannot be seen.
pub fn plan(
    config: &DotfilesConfig,
    source_exists: bool,
    may_need_privileges: bool,
) -> Result<Vec<Cmd>> {
    let mark = |cmd: Cmd| {
        if may_need_privileges {
            cmd.privileged()
        } else {
            cmd
        }
    };
    if !config.enabled {
        return Ok(Vec::new());
    }
    let Some(repo) = config.repo.as_deref() else {
        bail!("dotfiles are enabled but no repo is configured; set [dotfiles] repo");
    };

    if !source_exists {
        // Nothing on disk yet, so the clone has to happen whatever the
        // per-run apply preference says.
        return Ok(vec![mark(Cmd::new("chezmoi", ["init", "--apply", repo]))]);
    }
    if config.apply {
        return Ok(vec![mark(Cmd::new("chezmoi", ["apply"]))]);
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(repo: &str, apply: bool) -> DotfilesConfig {
        DotfilesConfig {
            enabled: true,
            repo: Some(repo.to_string()),
            apply,
        }
    }

    #[test]
    fn a_disabled_configuration_plans_nothing() {
        let cfg = DotfilesConfig {
            enabled: false,
            repo: Some("x".into()),
            apply: true,
        };

        assert!(plan(&cfg, false, false).unwrap().is_empty());
    }

    #[test]
    fn a_fresh_machine_is_initialised_and_applied_in_one_step() {
        let cmds = plan(
            &enabled("https://github.com/napalm255/dotfiles", true),
            false,
            false,
        )
        .unwrap();

        assert_eq!(cmds.len(), 1);
        assert_eq!(
            cmds[0].to_shell(),
            "chezmoi init --apply https://github.com/napalm255/dotfiles"
        );
    }

    #[test]
    fn an_existing_checkout_is_applied_not_reinitialised() {
        let cmds = plan(
            &enabled("https://github.com/napalm255/dotfiles", true),
            true,
            false,
        )
        .unwrap();

        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].to_shell(), "chezmoi apply");
    }

    #[test]
    fn an_existing_checkout_is_left_alone_when_apply_is_off() {
        let cmds = plan(
            &enabled("https://github.com/napalm255/dotfiles", false),
            true,
            false,
        )
        .unwrap();

        assert!(cmds.is_empty(), "got {cmds:?}");
    }

    #[test]
    fn a_fresh_machine_still_initialises_when_apply_is_off() {
        // Without the initial clone there are no dotfiles at all, so init has
        // to happen regardless of the per-run apply preference.
        let cmds = plan(
            &enabled("https://github.com/napalm255/dotfiles", false),
            false,
            false,
        )
        .unwrap();

        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].to_shell().starts_with("chezmoi init"));
    }

    #[test]
    fn enabled_without_a_repository_is_an_error() {
        let cfg = DotfilesConfig {
            enabled: true,
            repo: None,
            apply: true,
        };

        let err = plan(&cfg, false, false).unwrap_err();

        assert!(
            format!("{err:#}").contains("repo"),
            "the error should point at the missing repo: {err:#}"
        );
    }

    #[test]
    fn the_source_directory_is_the_chezmoi_default() {
        assert_eq!(
            source_dir(Path::new("/home/napalm")),
            PathBuf::from("/home/napalm/.local/share/chezmoi")
        );
    }

    #[test]
    fn a_step_that_may_need_privileges_is_marked() {
        let cmds = plan(&enabled("https://example.com/d", true), true, true).unwrap();

        assert!(
            cmds[0].privileged,
            "the run must know to ask for a password up front"
        );
    }

    #[test]
    fn an_ordinary_step_is_not_marked() {
        let cmds = plan(&enabled("https://example.com/d", true), true, false).unwrap();

        assert!(!cmds[0].privileged);
    }

    #[test]
    fn the_initial_clone_is_marked_too() {
        // `chezmoi init --apply` runs the same scripts as `chezmoi apply`.
        let cmds = plan(&enabled("https://example.com/d", true), false, true).unwrap();

        assert!(cmds[0].to_shell().starts_with("chezmoi init"));
        assert!(cmds[0].privileged);
    }
}
