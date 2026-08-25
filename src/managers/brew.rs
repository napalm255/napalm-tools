//! Homebrew formulae.
//!
//! Casks live in a separate namespace and are handled by
//! [`super::brew_cask`]. The two genuinely collide - the formula `copilot` is
//! the AWS ECS tool while the cask `copilot-cli` is GitHub Copilot - so they
//! are separate managers rather than a flag on one.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The Homebrew manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Brew;

/// Parse the output of `brew list --formula -1` into a set of formula names.
pub fn parse_list(output: &str) -> HashSet<String> {
    super::parse_lines(output)
}

/// Parse the output of `brew tap` into a set of tap names.
pub fn parse_taps(output: &str) -> HashSet<String> {
    super::parse_lines(output)
}

impl Brew {
    /// Explicitly-installed formulae, excluding those pulled in only as
    /// dependencies. Used for reporting, not for the installed-set diff:
    /// a formula present as a dependency is still present.
    pub fn leaves(&self) -> Result<HashSet<String>> {
        Ok(parse_list(&Cmd::new("brew", ["leaves"]).output()?))
    }
}

impl Manager for Brew {
    fn id(&self) -> ManagerId {
        ManagerId::Brew
    }

    fn binary(&self) -> &'static str {
        "brew"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        Ok(parse_list(
            &Cmd::new("brew", ["list", "--formula", "-1"]).output()?,
        ))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["install".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("brew", args)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["upgrade".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("brew", args)
    }

    fn installed_taps(&self) -> Result<HashSet<String>> {
        Ok(parse_taps(&Cmd::new("brew", ["tap"]).output()?))
    }

    fn tap_cmd(&self, tap: &str) -> Option<Cmd> {
        Some(Cmd::new("brew", ["tap", tap]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_formula_per_line() {
        let set = parse_list("bat\nfd\nripgrep\n");

        assert_eq!(
            set,
            HashSet::from(["bat".into(), "fd".into(), "ripgrep".into()])
        );
    }

    #[test]
    fn ignores_blank_lines_and_surrounding_whitespace() {
        let set = parse_list("  bat  \n\n\nfd\n   \n");

        assert_eq!(set, HashSet::from(["bat".into(), "fd".into()]));
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn parses_tap_names() {
        let set = parse_taps("homebrew/core\npowertmux/powertmux\n");

        assert!(set.contains("powertmux/powertmux"));
    }

    #[test]
    fn install_command_takes_every_package_at_once() {
        // One invocation, not one per package: brew start-up dominates.
        let cmd = Brew.install_cmd(&["bat".into(), "fd".into()]);

        assert_eq!(cmd.to_shell(), "brew install bat fd");
    }

    #[test]
    fn upgrade_command_is_distinct_from_install() {
        let cmd = Brew.upgrade_cmd(&["bat".into()]);

        assert_eq!(cmd.to_shell(), "brew upgrade bat");
    }

    #[test]
    fn tap_command_names_the_tap() {
        let cmd = Brew.tap_cmd("powertmux/powertmux").unwrap();

        assert_eq!(cmd.to_shell(), "brew tap powertmux/powertmux");
    }

    #[test]
    fn brew_is_usable_on_every_platform() {
        let atomic = Platform {
            fedora_family: true,
            atomic: true,
            wsl: false,
        };
        let wsl = Platform {
            fedora_family: true,
            atomic: false,
            wsl: true,
        };

        assert!(Brew.platform_ok(&atomic));
        assert!(Brew.platform_ok(&wsl));
    }
}
