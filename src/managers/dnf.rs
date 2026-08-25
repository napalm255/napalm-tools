//! dnf.
//!
//! Only ever available on a traditional, mutable Fedora install. On an
//! ostree-based system `dnf` is present on `PATH` and appears to work, but
//! anything it installs is discarded at the next OS update — so availability
//! is gated on the platform, never on the binary.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The dnf manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dnf;

/// Parse `rpm -qa --queryformat '%{NAME}\n'` output into package names.
pub fn parse_installed(output: &str) -> HashSet<String> {
    super::parse_lines(output)
}

impl Manager for Dnf {
    fn id(&self) -> ManagerId {
        ManagerId::Dnf
    }

    fn binary(&self) -> &'static str {
        "dnf"
    }

    fn platform_ok(&self, platform: &Platform) -> bool {
        !platform.atomic && platform.fedora_family
    }

    fn installed(&self) -> Result<HashSet<String>> {
        Ok(parse_installed(
            &Cmd::new("rpm", ["-qa", "--queryformat", "%{NAME}\n"]).output()?,
        ))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["install".to_string(), "-y".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("dnf", args)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec!["upgrade".to_string(), "-y".to_string()];
        args.extend(packages.iter().cloned());
        Cmd::new("dnf", args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATOMIC: Platform = Platform {
        fedora_family: true,
        atomic: true,
        wsl: false,
    };
    const PLAIN: Platform = Platform {
        fedora_family: true,
        atomic: false,
        wsl: false,
    };
    const UBUNTU: Platform = Platform {
        fedora_family: false,
        atomic: false,
        wsl: false,
    };

    #[test]
    fn dnf_is_never_usable_on_an_atomic_host() {
        // The single most important gate in the codebase. `dnf` IS on PATH
        // under Bluefin, so a PATH-based check would wrongly allow it.
        assert!(
            !Dnf.platform_ok(&ATOMIC),
            "dnf must be refused on an ostree-based system regardless of PATH"
        );
    }

    #[test]
    fn dnf_is_usable_on_a_traditional_fedora_host() {
        assert!(Dnf.platform_ok(&PLAIN));
    }

    #[test]
    fn dnf_is_not_used_outside_the_fedora_family() {
        assert!(!Dnf.platform_ok(&UBUNTU));
    }

    #[test]
    fn available_is_false_on_atomic_even_when_the_binary_exists() {
        // `available` combines the platform gate with a PATH lookup; on this
        // machine `dnf` really is on PATH, so this asserts the gate wins.
        assert!(!Dnf.available(&ATOMIC));
    }

    #[test]
    fn parses_package_names_from_rpm_output() {
        let set = parse_installed("bash\ncoreutils\nxdotool\n");

        assert!(set.contains("xdotool"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_installed("").is_empty());
    }
}
