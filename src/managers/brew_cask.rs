//! Homebrew casks.
//!
//! A separate manager rather than a flag on [`super::brew`], because formula
//! and cask names occupy different namespaces and genuinely collide: the
//! formula `copilot` is the AWS ECS tool, while the cask `copilot-cli` is
//! GitHub Copilot. Keeping the installed-sets apart removes the ambiguity.
//!
//! Casks are commonly described as macOS-only. Casks whose artifact is a plain
//! binary do install on Linux, which is how GitHub Copilot CLI and the Nerd
//! Fonts arrive. Casks that ship an application bundle or a pkg installer do
//! not; such a cask fails at install time and is reported as a command failure.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The Homebrew cask manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrewCask;

/// Parse `brew list --cask -1` output into cask names.
pub fn parse_list(output: &str) -> HashSet<String> {
    super::parse_lines(output)
}

impl Manager for BrewCask {
    fn id(&self) -> ManagerId {
        ManagerId::BrewCask
    }

    fn binary(&self) -> &'static str {
        "brew"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        Ok(parse_list(
            &Cmd::new("brew", ["list", "--cask", "-1"]).output()?,
        ))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["install".to_string(), "--cask".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("brew", args)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["upgrade".to_string(), "--cask".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("brew", args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_cask_per_line() {
        let set = parse_list("copilot-cli\nfont-fira-code-nerd-font\n");

        assert!(set.contains("copilot-cli"));
        assert!(set.contains("font-fira-code-nerd-font"));
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn install_passes_the_cask_flag() {
        let cmd = BrewCask.install_cmd(&["copilot-cli".into()]);

        assert_eq!(cmd.to_shell(), "brew install --cask copilot-cli");
    }

    #[test]
    fn upgrade_passes_the_cask_flag() {
        let cmd = BrewCask.upgrade_cmd(&["copilot-cli".into()]);

        assert_eq!(cmd.to_shell(), "brew upgrade --cask copilot-cli");
    }

    #[test]
    fn casks_are_a_distinct_manager_from_formulae() {
        // The `copilot` collision is why: same word, different software.
        assert_ne!(BrewCask.id(), ManagerId::Brew);
    }

    #[test]
    fn casks_never_declare_taps() {
        assert!(BrewCask.tap_cmd("whatever/tap").is_none());
    }
}
