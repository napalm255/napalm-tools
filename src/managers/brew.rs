//! Homebrew formulae.
//!
//! Casks live in a separate namespace and are handled by
//! [`super::brew_cask`]. The two genuinely collide - the formula `copilot` is
//! the AWS ECS tool while the cask `copilot-cli` is GitHub Copilot - so they
//! are separate managers rather than a flag on one.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId, parse_lines};
use crate::platform::Platform;

/// The Homebrew manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Brew;

/// Parse `brew trust --json v1` into the set of trusted tap paths.
pub fn parse_trusted(output: &str) -> HashSet<String> {
    if output.trim().is_empty() {
        return HashSet::new();
    }
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .as_ref()
        .and_then(|v| v.get("taps"))
        .and_then(|t| t.as_array())
        .map(|taps| {
            taps.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `tap` (a `user/repo` name) appears in a set of trusted paths.
///
/// Trust is recorded as a filesystem path, not a tap name, and a tap may live
/// outside Homebrew's own directory when it is a local checkout. Matching on
/// the `homebrew-<repo>` component covers both. Two taps from different users
/// sharing a repository name would collide; the cost of that is one idempotent
/// `brew trust` that reports the tap is already trusted.
pub fn tap_is_trusted(tap: &str, trusted: &HashSet<String>) -> bool {
    let Some((_user, repo)) = tap.split_once('/') else {
        return false;
    };
    let dir = format!("homebrew-{repo}");
    trusted
        .iter()
        .any(|path| path.rsplit('/').next() == Some(dir.as_str()))
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
        Ok(parse_lines(
            &Cmd::new("brew", ["list", "--formula", "-1"]).output()?,
        ))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("brew", &["install"], packages)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("brew", &["upgrade"], packages)
    }

    fn installed_taps(&self) -> Result<HashSet<String>> {
        Ok(parse_lines(&Cmd::new("brew", ["tap"]).output()?))
    }

    fn tap_cmd(&self, tap: &str) -> Option<Cmd> {
        Some(Cmd::new("brew", ["tap", tap]))
    }

    fn trust_cmd(&self, tap: &str) -> Option<Cmd> {
        Some(Cmd::new("brew", ["trust", "--tap", tap]))
    }

    fn trusted_taps(&self) -> Result<HashSet<String>> {
        Ok(parse_trusted(
            &Cmd::new("brew", ["trust", "--json", "v1"]).output()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_formula_per_line() {
        let set = parse_lines("bat\nfd\nripgrep\n");

        assert_eq!(
            set,
            HashSet::from(["bat".into(), "fd".into(), "ripgrep".into()])
        );
    }

    #[test]
    fn ignores_blank_lines_and_surrounding_whitespace() {
        let set = parse_lines("  bat  \n\n\nfd\n   \n");

        assert_eq!(set, HashSet::from(["bat".into(), "fd".into()]));
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_lines("").is_empty());
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
        use crate::platform::test_platforms::*;
        for p in [ATOMIC, PLAIN, SERVER, UNDER_WSL, CONTAINER] {
            assert!(Brew.platform_ok(&p), "{p:?}");
        }
    }

    const TRUST_JSON: &str = r#"{
      "taps": ["/var/home/napalm/git/homebrew-powertmux",
               "/home/linuxbrew/.linuxbrew/Homebrew/Library/Taps/openclaw/homebrew-tap"],
      "formulae": ["/var/home/napalm/git/homebrew-powertmux/powertmux"],
      "casks": [],
      "commands": []
    }"#;

    #[test]
    fn trusted_taps_are_parsed_from_json() {
        let set = parse_trusted(TRUST_JSON);

        assert_eq!(set.len(), 2, "got {set:?}");
        assert!(set.contains("/var/home/napalm/git/homebrew-powertmux"));
    }

    #[test]
    fn only_taps_are_taken_not_formulae() {
        // The formulae list holds paths too; conflating them would report a
        // tap as trusted because one of its formulae is.
        let set = parse_trusted(TRUST_JSON);

        assert!(
            !set.iter().any(|p| p.ends_with("/powertmux")),
            "got {set:?}"
        );
    }

    #[test]
    fn empty_trust_output_is_an_empty_set() {
        assert!(parse_trusted("").is_empty());
        assert!(parse_trusted(r#"{"taps":[]}"#).is_empty());
    }

    #[test]
    fn a_tap_in_homebrews_own_directory_is_recognised() {
        let trusted = parse_trusted(TRUST_JSON);

        assert!(tap_is_trusted("openclaw/tap", &trusted));
    }

    #[test]
    fn a_local_checkout_tap_is_recognised_too() {
        // This machine's powertmux tap points at ~/git/homebrew-powertmux,
        // outside Homebrew's directory entirely.
        let trusted = parse_trusted(TRUST_JSON);

        assert!(tap_is_trusted("powertmux/powertmux", &trusted));
    }

    #[test]
    fn an_untrusted_tap_is_not_recognised() {
        let trusted = parse_trusted(TRUST_JSON);

        assert!(!tap_is_trusted("someone/unrelated", &trusted));
    }

    #[test]
    fn trusting_a_tap_uses_the_tap_flag() {
        let cmd = Brew.trust_cmd("powertmux/powertmux").unwrap();

        assert_eq!(cmd.to_shell(), "brew trust --tap powertmux/powertmux");
    }
}
