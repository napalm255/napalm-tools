//! The version this binary is, and the versions a release announces.
//!
//! Hand-rolled rather than the `semver` crate because every version here is
//! produced by release-please as `MAJOR.MINOR.PATCH` with at most a simple
//! pre-release; comparators, ranges and build metadata are the rest of what
//! semver is, and none of it would ever be exercised. Anything outside that
//! subset is rejected rather than approximated: a version nt cannot order is
//! a version it must not act on.

use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::fmt;

/// A release version: `MAJOR.MINOR.PATCH` with an optional pre-release.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    /// Breaking changes.
    pub major: u64,
    /// Features.
    pub minor: u64,
    /// Fixes.
    pub patch: u64,
    /// Dot-separated pre-release identifiers; empty for a final release.
    pre: Vec<PreId>,
}

/// One dot-separated pre-release identifier.
///
/// Numeric identifiers compare as numbers, so `rc.2` precedes `rc.10`, and
/// sort before alphabetic ones, as semver requires.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PreId {
    Numeric(u64),
    Alpha(String),
}

impl Version {
    /// Parse `X.Y.Z` or `X.Y.Z-pre`, with an optional leading `v` so a git
    /// tag can be handed straight in.
    pub fn parse(text: &str) -> Result<Version> {
        let text = text.trim();
        let rest = text.strip_prefix('v').unwrap_or(text);
        if rest.contains('+') {
            bail!("{text:?} carries build metadata, which nt does not release");
        }
        let (numbers, pre) = match rest.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(pre)),
            None => (rest, None),
        };

        let mut parts = numbers.split('.');
        let mut number = |what: &str| -> Result<u64> {
            let Some(part) = parts.next() else {
                bail!("{text:?} has no {what} component");
            };
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                bail!("{text:?} has a {what} component that is not a number");
            }
            part.parse()
                .map_err(|_| anyhow::anyhow!("{text:?} has a {what} component that is too large"))
        };
        let major = number("major")?;
        let minor = number("minor")?;
        let patch = number("patch")?;
        if parts.next().is_some() {
            bail!("{text:?} has more than three components");
        }

        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                if pre.is_empty() {
                    bail!("{text:?} ends in a dash with no pre-release");
                }
                pre.split('.').map(PreId::parse).collect::<Result<_>>()?
            }
        };
        Ok(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// The version this binary was built as.
    ///
    /// Cargo guarantees the manifest parses, but a panic here would be a
    /// crash on start-up for a cosmetic field, so a malformed version falls
    /// back to `0.0.0` - which reads as "older than every release" and so
    /// errs towards offering an update rather than hiding one.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre: Vec::new(),
        })
    }

    /// Whether this is a pre-release.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl PreId {
    fn parse(part: &str) -> Result<PreId> {
        if part.is_empty() {
            bail!("a pre-release identifier is empty");
        }
        if part.bytes().all(|b| b.is_ascii_digit()) {
            return match part.parse() {
                Ok(n) => Ok(PreId::Numeric(n)),
                Err(_) => bail!("pre-release identifier {part:?} is too large"),
            };
        }
        if !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            bail!("pre-release identifier {part:?} is not alphanumeric");
        }
        Ok(PreId::Alpha(part.to_string()))
    }
}

impl Ord for Version {
    /// Written out rather than derived: deriving would compare `pre` as a
    /// `Vec`, where the empty one sorts first, making `1.0.0` older than
    /// `1.0.0-rc.1`. A pre-release precedes the release it leads to.
    fn cmp(&self, other: &Version) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    /// Without a leading `v`: that belongs to a tag, not to a version.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (i, id) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match id {
                PreId::Numeric(n) => write!(f, "{n}")?,
                PreId::Alpha(s) => f.write_str(s)?,
            }
        }
        Ok(())
    }
}

impl std::str::FromStr for Version {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Version> {
        Version::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("the test's own literal parses")
    }

    #[test]
    fn a_leading_v_is_stripped_so_a_git_tag_parses_as_a_version() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("v1.2.3").to_string(), "1.2.3");
    }

    #[test]
    fn versions_order_by_major_then_minor_then_patch() {
        assert!(v("2.0.0") > v("1.9.9"));
        assert!(v("1.2.0") > v("1.1.9"));
        assert!(v("1.1.2") > v("1.1.1"));
        assert!(v("0.1.0") < v("0.2.0"));
    }

    #[test]
    fn a_prerelease_sorts_before_the_release_it_precedes() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0") > v("1.0.0-rc.1"));
    }

    #[test]
    fn a_release_sorts_after_every_prerelease_of_itself() {
        for pre in ["1.0.0-alpha", "1.0.0-rc.99", "1.0.0-zzz"] {
            assert!(v(pre) < v("1.0.0"), "{pre}");
        }
    }

    #[test]
    fn numeric_prerelease_identifiers_compare_as_numbers_not_as_text() {
        assert!(v("1.0.0-rc.2") < v("1.0.0-rc.10"));
    }

    #[test]
    fn a_numeric_prerelease_identifier_sorts_before_an_alphabetic_one() {
        assert!(v("1.0.0-1") < v("1.0.0-alpha"));
    }

    #[test]
    fn a_longer_prerelease_sorts_after_its_own_prefix() {
        assert!(v("1.0.0-rc") < v("1.0.0-rc.1"));
    }

    #[test]
    fn two_prereleases_of_different_releases_order_by_the_release_first() {
        assert!(v("1.0.0-rc.9") < v("1.0.1-rc.1"));
    }

    #[test]
    fn equal_versions_are_neither_newer_nor_older() {
        assert_eq!(v("1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3").cmp(&v("1.2.3")), Ordering::Equal);
        assert_eq!(v("1.0.0-rc.1").cmp(&v("1.0.0-rc.1")), Ordering::Equal);
    }

    #[test]
    fn a_version_with_too_few_or_too_many_components_is_rejected() {
        for bad in ["1.2", "1", "1.2.3.4"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_non_numeric_component_is_rejected() {
        for bad in ["1.x.0", "1.2.z", "a.b.c", "1..0"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_empty_string_and_a_bare_v_are_rejected() {
        assert!(Version::parse("").is_err());
        assert!(Version::parse("v").is_err());
    }

    #[test]
    fn a_component_too_large_for_u64_is_an_error_not_a_panic() {
        let huge = format!("1.2.{}0", u64::MAX);

        assert!(Version::parse(&huge).is_err());
    }

    #[test]
    fn build_metadata_is_rejected_rather_than_silently_ignored() {
        // Ignoring it would make two different builds compare equal.
        assert!(Version::parse("1.2.3+abc").is_err());
    }

    #[test]
    fn an_empty_prerelease_after_the_dash_is_rejected() {
        assert!(Version::parse("1.2.3-").is_err());
        assert!(Version::parse("1.2.3-rc..1").is_err());
    }

    #[test]
    fn a_prerelease_identifier_outside_the_alphabet_is_rejected() {
        assert!(Version::parse("1.2.3-rc_1").is_err());
        assert!(Version::parse("1.2.3-rc 1").is_err());
    }

    #[test]
    fn a_prerelease_may_contain_a_dash() {
        assert_eq!(v("1.2.3-rc-1").to_string(), "1.2.3-rc-1");
    }

    #[test]
    fn parsing_arbitrary_junk_never_panics() {
        for junk in [
            "..",
            "-",
            "v-",
            "1.2.3-",
            "  ",
            "\n",
            "vvv1.2.3",
            "1.2.3-rc.",
            "٣.٢.١",
            "1.2.-3",
            "-1.2.3",
            "1.2.3.",
            ".1.2.3",
            "1.2.3+",
            "🦀",
        ] {
            let _ = Version::parse(junk);
        }
    }

    #[test]
    fn a_version_round_trips_through_display_and_parse() {
        for text in ["0.0.0", "1.2.3", "1.0.0-rc.1", "0.1.0-alpha.2.3"] {
            assert_eq!(v(text).to_string(), text);
            assert_eq!(v(&v(text).to_string()), v(text));
        }
    }

    #[test]
    fn the_version_this_binary_reports_parses_and_is_not_the_fallback() {
        // The fallback exists so a malformed manifest cannot panic on
        // start-up; it must never be what a real build reports.
        let current = Version::current();

        assert_eq!(current.to_string(), env!("CARGO_PKG_VERSION"));
        assert!(Version::parse(env!("CARGO_PKG_VERSION")).is_ok());
    }

    #[test]
    fn a_running_version_newer_than_the_latest_release_is_not_an_update() {
        // A locally built binary ahead of the last release.
        assert!(v("0.3.0") > v("0.2.0"));
        assert!(!(v("0.2.0") > v("0.2.0")));
    }

    #[test]
    fn a_prerelease_is_recognised_as_one() {
        assert!(v("1.0.0-rc.1").is_prerelease());
        assert!(!v("1.0.0").is_prerelease());
    }

    #[test]
    fn a_version_parses_through_the_from_str_impl() {
        let parsed: Version = "v1.2.3".parse().expect("parses");

        assert_eq!(parsed, v("1.2.3"));
    }
}
