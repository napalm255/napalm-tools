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

use super::{Cmd, Manager, ManagerId, parse_lines};
use crate::platform::Platform;

/// The Homebrew cask manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrewCask;

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
        Ok(parse_lines(
            &Cmd::new("brew", ["list", "--cask", "-1"]).output()?,
        ))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("brew", &["install", "--cask"], packages)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("brew", &["upgrade", "--cask"], packages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
