//! The compiled-in catalog of bundles and the packages they contain.
//!
//! `BUNDLES` is the single source of truth: CLI flags, `nt bundles`, and any
//! future UI all iterate it, so adding a bundle cannot leave the CLI behind.

pub mod catalog;

use crate::managers::ManagerId;
use crate::platform::{Platform, Platforms};

pub use catalog::BUNDLES;

/// One way of obtaining a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    /// Which manager supplies it.
    pub manager: ManagerId,
    /// The package's identifier within that manager.
    pub id: &'static str,
    /// Third-party Homebrew tap that must be added first, if any.
    pub tap: Option<&'static str>,
    /// Platforms this provider is usable on.
    pub platforms: Platforms,
}

impl Provider {
    /// A provider with no tap and no platform constraint.
    pub const fn new(manager: ManagerId, id: &'static str) -> Provider {
        Provider {
            manager,
            id,
            tap: None,
            platforms: Platforms::ALL,
        }
    }

    /// Same, but requiring a Homebrew tap.
    pub const fn tapped(id: &'static str, tap: &'static str) -> Provider {
        Provider {
            manager: ManagerId::Brew,
            id,
            tap: Some(tap),
            platforms: Platforms::ALL,
        }
    }

    /// Same, but constrained to certain platforms.
    pub const fn gated(manager: ManagerId, id: &'static str, platforms: Platforms) -> Provider {
        Provider {
            manager,
            id,
            tap: None,
            platforms,
        }
    }
}

/// A package, and the ordered list of ways to obtain it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pkg {
    /// Display name, used in output and in `[extra]` references.
    pub name: &'static str,
    /// The executable this package provides, when there is one.
    ///
    /// If it resolves on `PATH`, the package counts as satisfied whatever put
    /// it there. That covers three real cases: tools the OS image already
    /// ships, tools installed by a vendor script that self-updates, and
    /// packages whose formula name differs from the binary they install.
    /// Fonts and libraries leave this `None`.
    pub binary: Option<&'static str>,
    /// Ways to obtain the package, **most preferred first**. This ordering is
    /// how "brew first, dnf as a last resort" is expressed.
    pub providers: &'static [Provider],
}

impl Pkg {
    /// Select the provider to use, given the platform and which managers are
    /// usable on it.
    ///
    /// Walks providers in declared order and returns the first that is both
    /// permitted on this platform and backed by an available manager. `None`
    /// means the package cannot be obtained here — the caller reports it as
    /// unavailable rather than falling back to something unsafe.
    pub fn select<F>(&self, platform: &Platform, available: F) -> Option<&'static Provider>
    where
        F: Fn(ManagerId) -> bool,
    {
        self.providers
            .iter()
            .find(|p| p.platforms.matches(platform) && available(p.manager))
    }
}

/// A named, toggleable group of packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bundle {
    /// Kebab-case name; also the `--name` / `--no-name` CLI flag.
    pub name: &'static str,
    /// One-line description, shown in `--help` and `nt bundles`.
    pub description: &'static str,
    /// Whether the bundle is on when nothing says otherwise.
    pub default_enabled: bool,
    /// Platforms the bundle applies to.
    pub platforms: Platforms,
    /// Packages in the bundle.
    pub packages: &'static [Pkg],
}

/// Look a bundle up by name.
pub fn find(name: &str) -> Option<&'static Bundle> {
    BUNDLES.iter().find(|b| b.name == name)
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

    static BREW_THEN_DNF: &[Provider] = &[
        Provider::new(ManagerId::Brew, "ripgrep"),
        Provider::gated(ManagerId::Dnf, "ripgrep", Platforms::NOT_ATOMIC),
    ];

    static DNF_ONLY: &[Provider] = &[Provider::gated(
        ManagerId::Dnf,
        "xdotool",
        Platforms::NOT_ATOMIC,
    )];

    fn all_available(_: ManagerId) -> bool {
        true
    }

    #[test]
    fn selects_the_first_usable_provider() {
        let pkg = Pkg {
            name: "ripgrep",
            binary: None,
            providers: BREW_THEN_DNF,
        };

        let chosen = pkg.select(&PLAIN, all_available).unwrap();

        assert_eq!(chosen.manager, ManagerId::Brew);
    }

    #[test]
    fn falls_through_when_the_preferred_manager_is_unavailable() {
        let pkg = Pkg {
            name: "ripgrep",
            binary: None,
            providers: BREW_THEN_DNF,
        };

        let chosen = pkg.select(&PLAIN, |m| m != ManagerId::Brew).unwrap();

        assert_eq!(chosen.manager, ManagerId::Dnf);
    }

    #[test]
    fn skips_a_provider_barred_on_this_platform() {
        // The dnf provider is gated off on atomic, so even with the manager
        // reporting itself available there is nothing left to fall back to.
        let pkg = Pkg {
            name: "ripgrep",
            binary: None,
            providers: BREW_THEN_DNF,
        };

        let chosen = pkg.select(&ATOMIC, |m| m != ManagerId::Brew);

        assert!(
            chosen.is_none(),
            "dnf must never be selected on an atomic host, got {chosen:?}"
        );
    }

    #[test]
    fn yields_none_when_no_provider_applies() {
        // The xdotool case: dnf-only, and we are on an atomic host.
        let pkg = Pkg {
            name: "xdotool",
            binary: None,
            providers: DNF_ONLY,
        };

        assert!(pkg.select(&ATOMIC, all_available).is_none());
    }

    #[test]
    fn yields_the_dnf_provider_on_a_traditional_host() {
        let pkg = Pkg {
            name: "xdotool",
            binary: None,
            providers: DNF_ONLY,
        };

        let chosen = pkg.select(&PLAIN, all_available).unwrap();

        assert_eq!(chosen.manager, ManagerId::Dnf);
        assert_eq!(chosen.id, "xdotool");
    }

    #[test]
    fn find_returns_the_named_bundle() {
        let b = find("core").expect("core bundle must exist");

        assert_eq!(b.name, "core");
    }

    #[test]
    fn find_returns_none_for_an_unknown_name() {
        assert!(find("no-such-bundle").is_none());
    }
}
