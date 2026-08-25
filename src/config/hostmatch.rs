//! Matching `[host."..."]` patterns against the machine's hostname.

use anyhow::{Context, Result};
use globset::Glob;

/// Whether `hostname` matches the glob `pattern`.
///
/// Patterns are shell-style globs over the whole hostname. `*` spans dots, so
/// `*.example.com` matches a multi-label FQDN such as `a.b.example.com`.
pub fn matches(pattern: &str, hostname: &str) -> Result<bool> {
    let glob = Glob::new(pattern).with_context(|| format!("invalid host pattern {pattern:?}"))?;
    Ok(glob.compile_matcher().is_match(hostname))
}

/// Read this machine's hostname.
///
/// `NT_HOSTNAME` overrides, which keeps host-override behaviour testable
/// without renaming the machine.
pub fn hostname() -> String {
    if let Ok(h) = std::env::var("NT_HOSTNAME") {
        if !h.trim().is_empty() {
            return h.trim().to_string();
        }
    }
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_name_matches_itself() {
        assert!(matches("gibson", "gibson").unwrap());
    }

    #[test]
    fn an_exact_name_does_not_match_a_different_host() {
        assert!(!matches("gibson", "napalm-desktop").unwrap());
    }

    #[test]
    fn a_trailing_wildcard_matches_a_prefix() {
        assert!(matches("wsl-*", "wsl-fedora").unwrap());
        assert!(!matches("wsl-*", "fedora-wsl").unwrap());
    }

    #[test]
    fn a_leading_wildcard_spans_dots_in_an_fqdn() {
        // The real hostname on the development machine is a three-label FQDN;
        // a domain pattern has to reach across the intermediate label.
        assert!(
            matches("*.naponline.net", "napalm-desktop.local.naponline.net").unwrap(),
            "`*` must span dots so domain patterns work on an FQDN"
        );
    }

    #[test]
    fn a_domain_pattern_does_not_match_another_domain() {
        assert!(!matches("*.naponline.net", "host.example.com").unwrap());
    }

    #[test]
    fn a_bare_star_matches_everything() {
        assert!(matches("*", "anything.at.all").unwrap());
    }

    #[test]
    fn an_invalid_pattern_is_an_error_not_a_panic() {
        assert!(matches("[unclosed", "whatever").is_err());
    }
}
