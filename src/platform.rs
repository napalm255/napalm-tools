//! Detection of the Linux platform `nt` is running on.
//!
//! The distinction that matters most is `atomic`: on an ostree-based system
//! (Bluefin, Silverblue) `dnf` is present on `PATH` and appears to work, but
//! anything it installs is discarded on the next OS update. Every consumer
//! gates on this flag rather than on binary availability.

use std::fs;
use std::path::PathBuf;

/// The host platform, as far as package provisioning is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    /// Fedora or a Fedora derivative (matched via `ID` or `ID_LIKE`).
    pub fedora_family: bool,
    /// Booted from an ostree commit; the OS tree is immutable.
    pub atomic: bool,
    /// Running under the Windows Subsystem for Linux.
    pub wsl: bool,
}

/// The raw evidence platform detection is derived from.
///
/// Split out from [`Platform::detect`] so the decision logic can be tested
/// against fixtures without touching the filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evidence<'a> {
    /// Contents of `/etc/os-release`.
    pub os_release: &'a str,
    /// Contents of `/proc/sys/kernel/osrelease` (the kernel version string).
    pub kernel_osrelease: &'a str,
    /// Whether `/run/ostree-booted` exists.
    pub ostree_booted: bool,
    /// Value of `$WSL_DISTRO_NAME`, if set.
    pub wsl_distro_name: Option<&'a str>,
}

impl Platform {
    /// Derive a [`Platform`] from raw evidence. Pure; no I/O.
    pub fn from_evidence(ev: Evidence<'_>) -> Platform {
        let id = os_release_field(ev.os_release, "ID").unwrap_or_default();
        let id_like = os_release_field(ev.os_release, "ID_LIKE").unwrap_or_default();

        let fedora_family = id == "fedora" || id_like.split_whitespace().any(|w| w == "fedora");

        let wsl = ev.wsl_distro_name.is_some_and(|v| !v.is_empty())
            || ev
                .kernel_osrelease
                .to_ascii_lowercase()
                .contains("microsoft");

        Platform {
            fedora_family,
            atomic: ev.ostree_booted,
            wsl,
        }
    }
}

/// Extract a single `KEY=VALUE` field from `os-release` content, stripping
/// the optional surrounding quotes the format permits.
fn os_release_field(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

/// A platform constraint attached to a bundle or a package provider.
///
/// Constraints are exclusions rather than inclusions: the common case is
/// "everywhere except X", and stating it that way means a newly-supported
/// platform does not silently drop every existing entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Platforms {
    /// Unavailable on ostree-based systems.
    pub exclude_atomic: bool,
    /// Unavailable under WSL.
    pub exclude_wsl: bool,
}

impl Platforms {
    /// No constraint.
    pub const ALL: Platforms = Platforms {
        exclude_atomic: false,
        exclude_wsl: false,
    };
    /// Everywhere except ostree-based systems.
    pub const NOT_ATOMIC: Platforms = Platforms {
        exclude_atomic: true,
        exclude_wsl: false,
    };
    /// Everywhere except WSL.
    pub const NOT_WSL: Platforms = Platforms {
        exclude_atomic: false,
        exclude_wsl: true,
    };

    /// Whether this constraint admits `platform`.
    pub fn matches(&self, platform: &Platform) -> bool {
        !(self.exclude_atomic && platform.atomic) && !(self.exclude_wsl && platform.wsl)
    }
}

/// Filesystem locations platform detection reads from.
///
/// Overridable so detection can be exercised against fixtures.
#[derive(Debug, Clone)]
pub struct Sources {
    /// Path to `os-release`.
    pub os_release: PathBuf,
    /// Path to the kernel version string.
    pub kernel_osrelease: PathBuf,
    /// Path to the marker file whose presence indicates an ostree boot.
    pub ostree_marker: PathBuf,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            os_release: PathBuf::from("/etc/os-release"),
            kernel_osrelease: PathBuf::from("/proc/sys/kernel/osrelease"),
            ostree_marker: PathBuf::from("/run/ostree-booted"),
        }
    }
}

impl Sources {
    /// The real system paths, with `NT_OS_RELEASE` and `NT_OSTREE_MARKER`
    /// honoured as overrides so detection can be exercised end to end.
    pub fn system() -> Self {
        let mut s = Sources::default();
        if let Ok(p) = std::env::var("NT_OS_RELEASE") {
            s.os_release = PathBuf::from(p);
        }
        if let Ok(p) = std::env::var("NT_OSTREE_MARKER") {
            s.ostree_marker = PathBuf::from(p);
        }
        s
    }
}

impl Platform {
    /// Detect the platform this process is running on.
    pub fn detect() -> Platform {
        Platform::detect_from(&Sources::system(), std::env::var("WSL_DISTRO_NAME").ok())
    }

    /// Detect from explicit sources. A source that cannot be read is treated
    /// as absent rather than as an error: detection must never be the reason
    /// `nt` fails to start.
    pub fn detect_from(sources: &Sources, wsl_distro_name: Option<String>) -> Platform {
        let os_release = fs::read_to_string(&sources.os_release).unwrap_or_default();
        let kernel_osrelease = fs::read_to_string(&sources.kernel_osrelease).unwrap_or_default();
        Platform::from_evidence(Evidence {
            os_release: &os_release,
            kernel_osrelease: &kernel_osrelease,
            ostree_booted: sources.ostree_marker.exists(),
            wsl_distro_name: wsl_distro_name.as_deref(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUEFIN: &str = r#"NAME="Bluefin"
ID=bluefin
ID_LIKE="fedora"
VERSION_ID=44
VARIANT_ID=bluefin-dx-nvidia-open
"#;

    const FEDORA: &str = r#"NAME="Fedora Linux"
ID=fedora
VERSION_ID=44
"#;

    const UBUNTU: &str = r#"NAME="Ubuntu"
ID=ubuntu
ID_LIKE=debian
"#;

    #[test]
    fn bluefin_with_ostree_marker_is_atomic() {
        let p = Platform::from_evidence(Evidence {
            os_release: BLUEFIN,
            kernel_osrelease: "7.0.12-201.fc44.x86_64",
            ostree_booted: true,
            wsl_distro_name: None,
        });
        assert!(p.atomic, "ostree-booted marker must mean atomic");
    }

    #[test]
    fn bluefin_is_fedora_family_via_id_like() {
        // ID=bluefin, so only ID_LIKE can establish the family.
        let p = Platform::from_evidence(Evidence {
            os_release: BLUEFIN,
            kernel_osrelease: "7.0.12-201.fc44.x86_64",
            ostree_booted: true,
            wsl_distro_name: None,
        });
        assert!(p.fedora_family, "ID_LIKE=fedora must establish the family");
    }

    #[test]
    fn plain_fedora_without_ostree_marker_is_not_atomic() {
        let p = Platform::from_evidence(Evidence {
            os_release: FEDORA,
            kernel_osrelease: "6.11.0-1.fc44.x86_64",
            ostree_booted: false,
            wsl_distro_name: None,
        });
        assert!(!p.atomic);
        assert!(p.fedora_family);
    }

    #[test]
    fn ubuntu_is_not_fedora_family() {
        let p = Platform::from_evidence(Evidence {
            os_release: UBUNTU,
            kernel_osrelease: "6.8.0-generic",
            ostree_booted: false,
            wsl_distro_name: None,
        });
        assert!(!p.fedora_family);
    }

    #[test]
    fn microsoft_in_kernel_version_means_wsl() {
        let p = Platform::from_evidence(Evidence {
            os_release: FEDORA,
            kernel_osrelease: "5.15.153.1-microsoft-standard-WSL2",
            ostree_booted: false,
            wsl_distro_name: None,
        });
        assert!(p.wsl);
    }

    #[test]
    fn wsl_distro_name_env_means_wsl() {
        let p = Platform::from_evidence(Evidence {
            os_release: FEDORA,
            kernel_osrelease: "6.11.0-1.fc44.x86_64",
            ostree_booted: false,
            wsl_distro_name: Some("FedoraLinux-44"),
        });
        assert!(p.wsl);
    }

    #[test]
    fn ordinary_desktop_is_not_wsl() {
        let p = Platform::from_evidence(Evidence {
            os_release: FEDORA,
            kernel_osrelease: "6.11.0-1.fc44.x86_64",
            ostree_booted: false,
            wsl_distro_name: None,
        });
        assert!(!p.wsl);
    }

    /// Build a fixture source tree; `ostree` controls whether the marker exists.
    fn fixture(dir: &std::path::Path, os_release: &str, kernel: &str, ostree: bool) -> Sources {
        fs::write(dir.join("os-release"), os_release).unwrap();
        fs::write(dir.join("osrelease"), kernel).unwrap();
        if ostree {
            fs::write(dir.join("ostree-booted"), "").unwrap();
        }
        Sources {
            os_release: dir.join("os-release"),
            kernel_osrelease: dir.join("osrelease"),
            ostree_marker: dir.join("ostree-booted"),
        }
    }

    #[test]
    fn detect_from_reads_the_ostree_marker_off_disk() {
        let dir = tempfile::tempdir().unwrap();
        let sources = fixture(dir.path(), BLUEFIN, "7.0.12-201.fc44.x86_64", true);

        let p = Platform::detect_from(&sources, None);

        assert!(p.atomic);
        assert!(p.fedora_family);
        assert!(!p.wsl);
    }

    #[test]
    fn detect_from_treats_a_missing_marker_as_not_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let sources = fixture(dir.path(), FEDORA, "6.11.0-1.fc44.x86_64", false);

        let p = Platform::detect_from(&sources, None);

        assert!(!p.atomic);
    }

    #[test]
    fn detect_from_tolerates_unreadable_sources() {
        // Detection must never be the reason nt fails to start.
        let sources = Sources {
            os_release: PathBuf::from("/nonexistent/os-release"),
            kernel_osrelease: PathBuf::from("/nonexistent/osrelease"),
            ostree_marker: PathBuf::from("/nonexistent/ostree-booted"),
        };

        let p = Platform::detect_from(&sources, None);

        assert!(!p.fedora_family);
        assert!(!p.atomic);
        assert!(!p.wsl);
    }

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
    const UNDER_WSL: Platform = Platform {
        fedora_family: true,
        atomic: false,
        wsl: true,
    };

    #[test]
    fn unconstrained_matches_every_platform() {
        assert!(Platforms::ALL.matches(&ATOMIC));
        assert!(Platforms::ALL.matches(&PLAIN));
        assert!(Platforms::ALL.matches(&UNDER_WSL));
    }

    #[test]
    fn not_atomic_rejects_only_atomic() {
        assert!(!Platforms::NOT_ATOMIC.matches(&ATOMIC));
        assert!(Platforms::NOT_ATOMIC.matches(&PLAIN));
        assert!(Platforms::NOT_ATOMIC.matches(&UNDER_WSL));
    }

    #[test]
    fn not_wsl_rejects_only_wsl() {
        assert!(!Platforms::NOT_WSL.matches(&UNDER_WSL));
        assert!(Platforms::NOT_WSL.matches(&ATOMIC));
        assert!(Platforms::NOT_WSL.matches(&PLAIN));
    }

    #[test]
    fn exclusions_compose() {
        let both = Platforms {
            exclude_atomic: true,
            exclude_wsl: true,
        };
        assert!(!both.matches(&ATOMIC));
        assert!(!both.matches(&UNDER_WSL));
        assert!(both.matches(&PLAIN));
    }
}
