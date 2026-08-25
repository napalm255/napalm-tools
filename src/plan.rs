//! Turning resolved configuration into a list of actions.
//!
//! This module is pure: it takes a snapshot of the world and returns what
//! should happen. `--dry-run` renders the result and a real run executes it,
//! so the two cannot drift apart.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::bundles::BUNDLES;
use crate::config::Resolved;
use crate::managers::ManagerId;
use crate::platform::Platform;

/// What the managers reported about the current state of the machine.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Managers usable on this host.
    pub available: BTreeSet<ManagerId>,
    /// Packages each manager already has installed.
    pub installed: BTreeMap<ManagerId, HashSet<String>>,
    /// Homebrew taps already configured.
    pub taps: HashSet<String>,
    /// Catalog-declared binaries that resolve on `PATH`.
    pub binaries: HashSet<String>,
}

impl Snapshot {
    /// Whether `manager` reports `package` as installed.
    pub fn has(&self, manager: ManagerId, package: &str) -> bool {
        self.installed
            .get(&manager)
            .is_some_and(|set| set.contains(package))
    }

    /// Whether the package is already satisfied by an executable on `PATH`,
    /// regardless of which manager (if any) put it there.
    pub fn has_binary(&self, pkg: &crate::bundles::Pkg) -> bool {
        pkg.binary.is_some_and(|b| self.binaries.contains(b))
    }
}

/// One step in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Add a Homebrew tap before installing from it.
    Tap {
        /// Manager the tap belongs to.
        manager: ManagerId,
        /// Tap name.
        tap: String,
    },
    /// Install packages that are missing.
    Install {
        /// Manager to install with.
        manager: ManagerId,
        /// Package identifiers, in catalog order.
        packages: Vec<String>,
    },
    /// Upgrade packages that are already installed.
    Upgrade {
        /// Manager to upgrade with.
        manager: ManagerId,
        /// Package identifiers, in catalog order.
        packages: Vec<String>,
    },
}

impl Action {
    /// The manager this action runs against.
    pub fn manager(&self) -> ManagerId {
        match self {
            Action::Tap { manager, .. }
            | Action::Install { manager, .. }
            | Action::Upgrade { manager, .. } => *manager,
        }
    }

    /// The command that carries this action out.
    pub fn to_cmd(&self) -> crate::managers::Cmd {
        let manager = crate::managers::get(self.manager());
        match self {
            Action::Tap { tap, .. } => manager
                .tap_cmd(tap)
                .expect("tap actions are only produced for managers that support taps"),
            Action::Install { packages, .. } => manager.install_cmd(packages),
            Action::Upgrade { packages, .. } => manager.upgrade_cmd(packages),
        }
    }
}

/// A package that cannot be obtained on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unavailable {
    /// Package display name.
    pub package: String,
    /// Bundle it came from, or `extra`.
    pub source: String,
    /// Why it cannot be provisioned.
    pub reason: String,
}

/// A bundle that was enabled but does not apply to this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedBundle {
    /// Bundle name.
    pub name: String,
    /// Why it was skipped.
    pub reason: String,
}

/// The full result of planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionPlan {
    /// Steps to run, in order.
    pub actions: Vec<Action>,
    /// Packages with no provider here.
    pub unavailable: Vec<Unavailable>,
    /// Bundles enabled but not applicable to this host.
    pub skipped: Vec<SkippedBundle>,
    /// Packages already present, requiring nothing.
    pub satisfied: Vec<String>,
    /// Dotfiles commands to run once packages have converged.
    pub dotfiles: Vec<crate::managers::Cmd>,
}

impl ActionPlan {
    /// Whether the plan would change anything.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.dotfiles.is_empty()
    }
}

/// Build a plan. Pure; performs no I/O.
pub fn build(resolved: &Resolved, platform: &Platform, snapshot: &Snapshot) -> ActionPlan {
    let mut plan = ActionPlan::default();

    // Accumulated per manager so each ends up with a single command.
    let mut to_install: BTreeMap<ManagerId, Vec<String>> = BTreeMap::new();
    let mut to_upgrade: BTreeMap<ManagerId, Vec<String>> = BTreeMap::new();
    let mut taps: Vec<String> = Vec::new();

    for bundle in BUNDLES {
        if !resolved.bundle_enabled(bundle.name) {
            continue;
        }
        if !bundle.platforms.matches(platform) {
            plan.skipped.push(SkippedBundle {
                name: bundle.name.to_string(),
                reason: "not applicable to this platform".to_string(),
            });
            continue;
        }

        for pkg in bundle.packages {
            let provider = pkg.select(platform, |m| snapshot.available.contains(&m));

            // A manager that owns the package takes precedence, because only
            // then is an upgrade meaningful.
            if let Some(p) = provider {
                if snapshot.has(p.manager, p.id) {
                    plan.satisfied.push(pkg.name.to_string());
                    if resolved.upgrade {
                        push_unique(to_upgrade.entry(p.manager).or_default(), p.id);
                    }
                    continue;
                }
            }

            // Otherwise an executable on PATH settles it, whatever put it
            // there: the OS image, a vendor installer, another manager. No
            // upgrade is planned, since nothing here owns it.
            if snapshot.has_binary(pkg) {
                plan.satisfied.push(pkg.name.to_string());
                continue;
            }

            let Some(provider) = provider else {
                plan.unavailable.push(Unavailable {
                    package: pkg.name.to_string(),
                    source: bundle.name.to_string(),
                    reason: unavailable_reason(pkg, platform),
                });
                continue;
            };

            if let Some(tap) = provider.tap {
                if !snapshot.taps.contains(tap) && !taps.iter().any(|t| t == tap) {
                    taps.push(tap.to_string());
                }
            }
            push_unique(to_install.entry(provider.manager).or_default(), provider.id);
        }
    }

    // Extras: outside the catalog, so there is no provider list to walk. The
    // manager is named directly and must be usable here.
    for (manager, packages) in &resolved.extra {
        for name in packages {
            if !snapshot.available.contains(manager) {
                plan.unavailable.push(Unavailable {
                    package: name.clone(),
                    source: "extra".to_string(),
                    reason: format!("{manager} is not available on this host"),
                });
                continue;
            }
            if snapshot.has(*manager, name) {
                plan.satisfied.push(name.clone());
                if resolved.upgrade {
                    push_unique(to_upgrade.entry(*manager).or_default(), name);
                }
                continue;
            }
            push_unique(to_install.entry(*manager).or_default(), name);
        }
    }

    // Taps first, so an install from a new tap can succeed.
    for tap in taps {
        plan.actions.push(Action::Tap {
            manager: ManagerId::Brew,
            tap,
        });
    }
    for (manager, packages) in to_install {
        plan.actions.push(Action::Install { manager, packages });
    }
    for (manager, packages) in to_upgrade {
        plan.actions.push(Action::Upgrade { manager, packages });
    }

    plan
}

/// Append `value` unless it is already present, preserving catalog order.
fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|v| v == value) {
        list.push(value.to_string());
    }
}

/// Explain why no provider could be selected for `pkg`.
fn unavailable_reason(pkg: &crate::bundles::Pkg, platform: &Platform) -> String {
    // Distinguish "barred by policy here" from "manager simply not installed",
    // because the remedy is different.
    let barred_by_platform = pkg.providers.iter().any(|p| !p.platforms.matches(platform));
    if barred_by_platform && platform.atomic {
        "no user-space provider on an atomic host".to_string()
    } else if barred_by_platform {
        "no provider available on this platform".to_string()
    } else {
        let wanted: Vec<&str> = pkg.providers.iter().map(|p| p.manager.as_str()).collect();
        format!("no available manager among: {}", wanted.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CliOverrides, ConfigFile, resolve};

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

    /// Resolve a config from TOML text, for the given host.
    fn cfg(text: &str) -> Resolved {
        resolve(
            &ConfigFile::parse(text).unwrap(),
            "testhost",
            &CliOverrides::default(),
        )
        .unwrap()
    }

    /// A configuration with exactly the named bundles enabled.
    ///
    /// Written against the catalog rather than a literal TOML string so these
    /// tests keep testing the plan builder when bundles are added or renamed.
    fn only(enabled: &[&str]) -> Resolved {
        let mut toml = String::from("[bundles]\n");
        for b in BUNDLES {
            toml.push_str(&format!("{} = {}\n", b.name, enabled.contains(&b.name)));
        }
        cfg(&toml)
    }

    /// A TOML fragment turning every bundle off.
    fn none_toml() -> String {
        let mut toml = String::from("[bundles]\n");
        for b in BUNDLES {
            toml.push_str(&format!("{} = false\n", b.name));
        }
        toml
    }

    /// Every bundle enabled.
    fn all_bundles_on() -> Resolved {
        let names: Vec<&str> = BUNDLES.iter().map(|b| b.name).collect();
        only(&names)
    }

    /// Only `core`, with upgrades turned on.
    fn upgrade_only_core() -> Resolved {
        let mut toml = none_toml();
        toml = toml.replace("core = false", "core = true");
        toml.push_str("\n[options]\nupgrade = true\n");
        cfg(&toml)
    }

    /// Every provider id the named bundle would install via `manager`.
    fn ids_in(bundle: &str, manager: ManagerId) -> HashSet<String> {
        BUNDLES
            .iter()
            .filter(|b| b.name == bundle)
            .flat_map(|b| b.packages)
            .flat_map(|p| p.providers)
            .filter(|pr| pr.manager == manager)
            .map(|pr| pr.id.to_string())
            .collect()
    }

    /// A snapshot where the named managers are available and nothing is installed.
    fn snapshot(available: &[ManagerId]) -> Snapshot {
        Snapshot {
            available: available.iter().copied().collect(),
            installed: BTreeMap::new(),
            taps: HashSet::new(),
            binaries: HashSet::new(),
        }
    }

    /// Packages an action installs with a given manager, if any.
    fn installs_for(plan: &ActionPlan, manager: ManagerId) -> Option<Vec<String>> {
        plan.actions.iter().find_map(|a| match a {
            Action::Install {
                manager: m,
                packages,
            } if *m == manager => Some(packages.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_missing_package_in_an_enabled_bundle_is_installed() {
        // Only `core` on, nothing installed.
        let plan = build(&only(&["core"]), &PLAIN, &snapshot(&[ManagerId::Brew]));

        let brew = installs_for(&plan, ManagerId::Brew).expect("expected a brew install");
        assert!(brew.contains(&"ripgrep".to_string()), "got {brew:?}");
    }

    #[test]
    fn an_already_installed_package_produces_no_action() {
        // The idempotency guarantee: a converged machine plans nothing.
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed
            .insert(ManagerId::Brew, ids_in("core", ManagerId::Brew));
        snap.taps.insert("powertmux/powertmux".to_string());

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            plan.is_empty(),
            "expected no actions, got {:?}",
            plan.actions
        );
        assert!(plan.satisfied.contains(&"ripgrep".to_string()));
    }

    #[test]
    fn a_disabled_bundle_contributes_nothing() {
        let plan = build(&only(&[]), &PLAIN, &snapshot(&[ManagerId::Brew]));

        assert!(plan.is_empty(), "got {:?}", plan.actions);
    }

    #[test]
    fn a_bundle_barred_on_this_platform_is_skipped_not_installed() {
        // `desktop` is enabled but declared unavailable under WSL.
        let plan = build(
            &only(&["desktop"]),
            &UNDER_WSL,
            &snapshot(&[ManagerId::Brew, ManagerId::Flatpak]),
        );

        assert!(plan.is_empty(), "got {:?}", plan.actions);
        assert!(
            plan.skipped.iter().any(|s| s.name == "desktop"),
            "the skip should be reported, got {:?}",
            plan.skipped
        );
    }

    #[test]
    fn a_package_with_no_provider_here_is_reported_unavailable() {
        // xdotool is dnf-only, and dnf is barred on an atomic host.
        let plan = build(
            &only(&["desktop"]),
            &ATOMIC,
            &snapshot(&[ManagerId::Brew, ManagerId::Flatpak]),
        );

        assert!(
            plan.unavailable.iter().any(|u| u.package == "xdotool"),
            "expected xdotool unavailable, got {:?}",
            plan.unavailable
        );
    }

    #[test]
    fn no_dnf_action_is_ever_planned_on_an_atomic_host() {
        // The whole-plan invariant, not just the one package.
        let plan = build(&all_bundles_on(), &ATOMIC, &snapshot(ManagerId::ALL));

        for action in &plan.actions {
            let manager = match action {
                Action::Tap { manager, .. }
                | Action::Install { manager, .. }
                | Action::Upgrade { manager, .. } => manager,
            };
            assert_ne!(*manager, ManagerId::Dnf, "planned a dnf action on atomic");
        }
    }

    #[test]
    fn a_tapped_package_is_preceded_by_its_tap() {
        let plan = build(
            &only(&["core"]),
            &PLAIN,
            &snapshot(&[ManagerId::Brew, ManagerId::Npm]),
        );

        let tap_at = plan
            .actions
            .iter()
            .position(|a| matches!(a, Action::Tap { tap, .. } if tap == "powertmux/powertmux"))
            .expect("expected a tap action");
        let install_at = plan
            .actions
            .iter()
            .position(|a| {
                matches!(
                    a,
                    Action::Install {
                        manager: ManagerId::Brew,
                        ..
                    }
                )
            })
            .expect("expected a brew install");

        assert!(tap_at < install_at, "the tap must come before the install");
    }

    #[test]
    fn an_existing_tap_is_not_added_again() {
        let mut snap = snapshot(&[ManagerId::Brew, ManagerId::Npm]);
        snap.taps.insert("powertmux/powertmux".to_string());

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            !plan.actions.iter().any(|a| matches!(a, Action::Tap { .. })),
            "tap already present, got {:?}",
            plan.actions
        );
    }

    #[test]
    fn installs_are_grouped_into_one_command_per_manager() {
        let plan = build(
            &only(&["core", "security"]),
            &PLAIN,
            &snapshot(&[ManagerId::Brew, ManagerId::Npm]),
        );

        let brew_installs = plan
            .actions
            .iter()
            .filter(|a| {
                matches!(
                    a,
                    Action::Install {
                        manager: ManagerId::Brew,
                        ..
                    }
                )
            })
            .count();

        assert_eq!(brew_installs, 1, "expected a single grouped brew install");
    }

    #[test]
    fn installed_packages_are_left_alone_without_the_upgrade_option() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed
            .insert(ManagerId::Brew, HashSet::from(["ripgrep".to_string()]));

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::Upgrade { .. })),
            "no upgrade should be planned by default"
        );
    }

    #[test]
    fn the_upgrade_option_upgrades_installed_packages() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed
            .insert(ManagerId::Brew, ids_in("core", ManagerId::Brew));
        snap.taps.insert("powertmux/powertmux".to_string());

        let plan = build(&upgrade_only_core(), &PLAIN, &snap);

        let upgraded = plan.actions.iter().find_map(|a| match a {
            Action::Upgrade { packages, .. } => Some(packages.clone()),
            _ => None,
        });

        assert!(
            upgraded.is_some_and(|p| p.contains(&"ripgrep".to_string())),
            "expected ripgrep to be upgraded, got {:?}",
            plan.actions
        );
    }

    #[test]
    fn extra_packages_are_planned_alongside_the_catalog() {
        let plan = build(
            &cfg(&format!("{}\n[extra]\nbrew = [\"jless\"]\n", none_toml())),
            &PLAIN,
            &snapshot(&[ManagerId::Brew]),
        );

        let brew = installs_for(&plan, ManagerId::Brew).expect("expected a brew install");
        assert_eq!(brew, vec!["jless".to_string()]);
    }

    #[test]
    fn an_extra_for_an_unavailable_manager_is_reported_not_dropped() {
        let plan = build(
            &cfg(&format!("{}\n[extra]\ndnf = [\"xdotool\"]\n", none_toml())),
            &ATOMIC,
            &snapshot(&[ManagerId::Brew]),
        );

        assert!(
            plan.unavailable.iter().any(|u| u.package == "xdotool"),
            "got {:?}",
            plan.unavailable
        );
    }

    #[test]
    fn an_already_installed_extra_is_not_reinstalled() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed
            .insert(ManagerId::Brew, HashSet::from(["jless".to_string()]));

        let plan = build(
            &cfg(&format!("{}\n[extra]\nbrew = [\"jless\"]\n", none_toml())),
            &PLAIN,
            &snap,
        );

        assert!(plan.is_empty(), "got {:?}", plan.actions);
    }

    #[test]
    fn a_system_scope_flatpak_is_not_reinstalled() {
        // Mirrors the real machine: the app exists, installed system-wide.
        let mut snap = snapshot(&[ManagerId::Brew, ManagerId::Flatpak]);
        snap.installed.insert(
            ManagerId::Flatpak,
            HashSet::from([
                "com.spotify.Client".to_string(),
                "org.remmina.Remmina".to_string(),
            ]),
        );

        let plan = build(&only(&["desktop"]), &PLAIN, &snap);

        assert!(
            installs_for(&plan, ManagerId::Flatpak).is_none(),
            "already-installed flatpaks must not be reinstalled, got {:?}",
            plan.actions
        );
    }

    #[test]
    fn an_install_action_renders_its_managers_command() {
        let action = Action::Install {
            manager: ManagerId::Brew,
            packages: vec!["bat".into(), "fd".into()],
        };

        assert_eq!(action.to_cmd().to_shell(), "brew install bat fd");
    }

    #[test]
    fn a_tap_action_renders_a_tap_command() {
        let action = Action::Tap {
            manager: ManagerId::Brew,
            tap: "powertmux/powertmux".into(),
        };

        assert_eq!(action.to_cmd().to_shell(), "brew tap powertmux/powertmux");
    }

    #[test]
    fn an_upgrade_action_renders_an_upgrade_command() {
        let action = Action::Upgrade {
            manager: ManagerId::Npm,
            packages: vec!["openclaw".into()],
        };

        assert_eq!(action.to_cmd().to_shell(), "npm update -g openclaw");
    }

    #[test]
    fn a_plan_carrying_only_a_dotfiles_step_is_not_empty() {
        let plan = ActionPlan {
            dotfiles: vec![crate::managers::Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        assert!(!plan.is_empty());
    }

    static VIM: &[crate::bundles::Provider] =
        &[crate::bundles::Provider::new(ManagerId::Brew, "vim")];

    #[test]
    fn a_binary_already_on_path_satisfies_the_package() {
        // The OS-image case: Bluefin ships vim, so brew must not install it.
        let pkg = crate::bundles::Pkg {
            name: "vim",
            binary: Some("vim"),
            providers: VIM,
        };
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.binaries.insert("vim".to_string());

        assert!(snap.has_binary(&pkg));
    }

    #[test]
    fn a_package_satisfied_by_path_is_not_installed() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        // Everything core declares resolves on PATH already.
        for b in BUNDLES.iter().filter(|b| b.name == "core") {
            for p in b.packages {
                if let Some(bin) = p.binary {
                    snap.binaries.insert(bin.to_string());
                }
            }
        }

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            plan.is_empty(),
            "expected nothing to do, got {:?}",
            plan.actions
        );
    }

    #[test]
    fn a_package_with_no_binary_declared_still_uses_the_manager() {
        // Fonts have no executable; only the cask listing can satisfy them.
        let pkg = crate::bundles::Pkg {
            name: "font-fira-code-nerd-font",
            binary: None,
            providers: VIM,
        };
        let snap = snapshot(&[ManagerId::Brew]);

        assert!(!snap.has_binary(&pkg));
    }
}
