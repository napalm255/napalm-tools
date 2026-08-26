//! Turning resolved configuration into a list of actions.
//!
//! This module is pure: it takes a snapshot of the world and returns what
//! should happen. `--dry-run` renders the result and a real run executes it,
//! so the two cannot drift apart.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::bundles::{BUNDLES, Pkg};
use crate::config::Resolved;
use crate::managers::{self, Cmd, ManagerId};
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
    /// Homebrew taps already trusted, as recorded paths.
    pub trusted_taps: HashSet<String>,
    /// Flatpak remotes configured in the user scope.
    pub remotes: HashSet<String>,
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
    pub fn has_binary(&self, pkg: &Pkg) -> bool {
        pkg.binary.is_some_and(|b| self.binaries.contains(b))
    }
}

/// One step in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Add the remote a manager installs from. Flatpak's user scope has none
    /// until one is added.
    AddRemote {
        /// The manager.
        manager: ManagerId,
    },
    /// Add a Homebrew tap before installing from it.
    Tap {
        /// Manager the tap belongs to.
        manager: ManagerId,
        /// Tap name.
        tap: String,
    },
    /// Trust a tap, without which Homebrew ignores its formulae entirely.
    Trust {
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
            Action::AddRemote { manager }
            | Action::Tap { manager, .. }
            | Action::Trust { manager, .. }
            | Action::Install { manager, .. }
            | Action::Upgrade { manager, .. } => *manager,
        }
    }

    /// The command that carries this action out.
    pub fn to_cmd(&self) -> Cmd {
        let manager = managers::get(self.manager());
        match self {
            Action::AddRemote { .. } => manager
                .add_remote_cmd()
                .expect("remote actions are only produced for managers that have remotes"),
            Action::Tap { tap, .. } => manager
                .tap_cmd(tap)
                .expect("tap actions are only produced for managers that support taps"),
            Action::Trust { tap, .. } => manager
                .trust_cmd(tap)
                .expect("trust actions are only produced for managers that support them"),
            Action::Install { packages, .. } => manager.install_cmd(packages),
            Action::Upgrade { packages, .. } => manager.upgrade_cmd(packages),
        }
    }
}

/// Where a wanted package stands on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The chosen manager reports it installed.
    Installed,
    /// Its executable is on `PATH`, put there by something other than the
    /// chosen manager: the OS image, a vendor script, another manager.
    OnPath,
    /// Not present; an action installs it.
    Missing,
    /// No provider can supply it here.
    Unavailable(String),
}

/// One wanted package and its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageState {
    /// Bundle it came from, or `extra`.
    pub bundle: String,
    /// Package display name.
    pub name: String,
    /// The provider that would be used, as `manager` and id.
    pub provider: Option<(ManagerId, String)>,
    /// Where it stands.
    pub state: State,
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
    /// Commands that make the managers themselves available. Run before the
    /// snapshot is taken, so they are decided separately and listed first.
    pub bootstrap: Vec<Cmd>,
    /// Steps to run, in order.
    pub actions: Vec<Action>,
    /// Every wanted package and where it stands.
    pub packages: Vec<PackageState>,
    /// Bundles enabled but not applicable to this host.
    pub skipped: Vec<SkippedBundle>,
    /// Dotfiles commands to run once packages have converged.
    pub dotfiles: Vec<Cmd>,
}

impl ActionPlan {
    /// Whether the plan would change anything.
    pub fn is_empty(&self) -> bool {
        self.bootstrap.is_empty() && self.actions.is_empty() && self.dotfiles.is_empty()
    }

    /// Packages with no provider here.
    pub fn unavailable(&self) -> Vec<Unavailable> {
        self.packages
            .iter()
            .filter_map(|p| match &p.state {
                State::Unavailable(reason) => Some(Unavailable {
                    package: p.name.clone(),
                    source: p.bundle.clone(),
                    reason: reason.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Names of packages already present, requiring nothing.
    pub fn satisfied(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|p| matches!(p.state, State::Installed | State::OnPath))
            .map(|p| p.name.clone())
            .collect()
    }

    /// Names of packages an action will install.
    pub fn missing(&self) -> Vec<String> {
        self.packages
            .iter()
            .filter(|p| p.state == State::Missing)
            .map(|p| p.name.clone())
            .collect()
    }

    /// Every command the plan would run, in order.
    pub fn commands(&self) -> Vec<Cmd> {
        self.bootstrap
            .iter()
            .cloned()
            .chain(self.actions.iter().map(Action::to_cmd))
            .chain(self.dotfiles.iter().cloned())
            .collect()
    }
}

/// Build a plan. Pure; performs no I/O.
pub fn build(resolved: &Resolved, platform: &Platform, snapshot: &Snapshot) -> ActionPlan {
    let mut plan = ActionPlan::default();

    // Accumulated per manager so each ends up with a single command.
    let mut to_install: BTreeMap<ManagerId, Vec<String>> = BTreeMap::new();
    let mut to_upgrade: BTreeMap<ManagerId, Vec<String>> = BTreeMap::new();
    let mut taps: Vec<String> = Vec::new();
    let mut trusts: Vec<String> = Vec::new();

    for bundle in BUNDLES {
        if !resolved.bundle_enabled(bundle.name) {
            tracing::debug!(bundle = bundle.name, "disabled");
            continue;
        }
        if let Some(reason) = bundle.platforms.rejection(platform) {
            tracing::debug!(bundle = bundle.name, reason, "skipped");
            plan.skipped.push(SkippedBundle {
                name: bundle.name.to_string(),
                reason: reason.to_string(),
            });
            continue;
        }

        for pkg in bundle.wanted(&resolved.prompt) {
            let provider = pkg.select(platform, |m| snapshot.available.contains(&m));
            let mut entry = PackageState {
                bundle: bundle.name.to_string(),
                name: pkg.name.to_string(),
                provider: provider.map(|p| (p.manager, p.id.to_string())),
                state: State::Missing,
            };

            // A manager that owns the package takes precedence, because only
            // then is an upgrade meaningful.
            if let Some(p) = provider
                && snapshot.has(p.manager, p.id)
            {
                entry.state = State::Installed;
                plan.packages.push(entry);
                if resolved.upgrade {
                    push_unique(to_upgrade.entry(p.manager).or_default(), p.id);
                }
                continue;
            }

            // Otherwise an executable on PATH settles it, whatever put it
            // there. No upgrade is planned, since nothing here owns it.
            if snapshot.has_binary(pkg) {
                entry.state = State::OnPath;
                plan.packages.push(entry);
                continue;
            }

            let Some(provider) = provider else {
                entry.state = State::Unavailable(unavailable_reason(pkg, platform));
                plan.packages.push(entry);
                continue;
            };

            if let Some(tap) = provider.tap {
                if !snapshot.taps.contains(tap) && !taps.iter().any(|t| t == tap) {
                    taps.push(tap.to_string());
                }
                // Separate from tapping: a tap can be present but untrusted,
                // in which case Homebrew ignores its formulae and the install
                // quietly does nothing.
                if !managers::brew::tap_is_trusted(tap, &snapshot.trusted_taps)
                    && !trusts.iter().any(|t| t == tap)
                {
                    trusts.push(tap.to_string());
                }
            }
            push_unique(to_install.entry(provider.manager).or_default(), provider.id);
            plan.packages.push(entry);
        }
    }

    // Extras: outside the catalog, so there is no provider list to walk. The
    // manager is named directly and must be usable here.
    for (manager, packages) in &resolved.extra {
        for name in packages {
            let mut entry = PackageState {
                bundle: "extra".to_string(),
                name: name.clone(),
                provider: Some((*manager, name.clone())),
                state: State::Missing,
            };
            if !snapshot.available.contains(manager) {
                entry.provider = None;
                entry.state =
                    State::Unavailable(format!("{manager} is not available on this host"));
            } else if snapshot.has(*manager, name) {
                entry.state = State::Installed;
                if resolved.upgrade {
                    push_unique(to_upgrade.entry(*manager).or_default(), name);
                }
            } else {
                push_unique(to_install.entry(*manager).or_default(), name);
            }
            plan.packages.push(entry);
        }
    }

    // Remote, tap, trust, then install: each is a precondition of the next.
    for manager in to_install.keys() {
        let m = managers::get(*manager);
        if let Some(remote) = m.remote_name()
            && !snapshot.remotes.contains(remote)
        {
            plan.actions.push(Action::AddRemote { manager: *manager });
        }
    }
    for tap in taps {
        plan.actions.push(Action::Tap {
            manager: ManagerId::Brew,
            tap,
        });
    }
    for tap in trusts {
        plan.actions.push(Action::Trust {
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
fn unavailable_reason(pkg: &Pkg, platform: &Platform) -> String {
    // Distinguish "barred by policy here" from "manager simply not installed",
    // because the remedy is different.
    let rejection = pkg
        .providers
        .iter()
        .find_map(|p| p.platforms.rejection(platform));
    match rejection {
        Some(reason) if platform.atomic && reason.contains("atomic") => {
            "no user-space provider on an atomic host".to_string()
        }
        Some(reason) => reason.to_string(),
        None => {
            let wanted: Vec<&str> = pkg.providers.iter().map(|p| p.manager.as_str()).collect();
            format!("no available manager among: {}", wanted.join(", "))
        }
    }
}

/// What the bootstrap phase found on the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Probe {
    /// Whether Homebrew is present.
    pub brew: bool,
    /// Whether mise is present.
    pub mise: bool,
    /// Whether `sudo` is present, without which dnf cannot be used.
    pub sudo: bool,
}

/// Packages Homebrew's installer needs on a Fedora host without them.
pub const BREW_PREREQUISITES: &[&str] = &["procps-ng", "curl", "file", "git", "gcc"];

/// Where Homebrew's installer is fetched from.
pub const BREW_INSTALLER: &str =
    "https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh";

/// The commands that make the managers available, and the managers they
/// will make available. Pure.
///
/// Homebrew is bootstrapped wherever it is missing: its installer works on
/// atomic hosts too, since `/home` is writable there. Its `dnf`
/// prerequisites are only planned where dnf is usable. mise comes from
/// Homebrew once that exists.
pub fn bootstrap(platform: &Platform, probe: Probe) -> (Vec<Cmd>, Vec<ManagerId>) {
    let mut cmds = Vec::new();
    let mut becomes_available = Vec::new();

    if !probe.brew {
        if managers::dnf::Dnf::usable_for_bootstrap(platform) && probe.sudo {
            cmds.push(
                Cmd::with_packages(
                    "sudo",
                    &["dnf", "install", "-y"],
                    &BREW_PREREQUISITES
                        .iter()
                        .map(|p| (*p).to_string())
                        .collect::<Vec<_>>(),
                )
                .privileged(),
            );
        }
        // The official installer, run non-interactively. It uses sudo itself
        // to create /home/linuxbrew, so the step keeps the terminal.
        cmds.push(
            Cmd::new(
                "bash",
                [
                    "-c",
                    &format!("curl -fsSL {BREW_INSTALLER} | NONINTERACTIVE=1 bash"),
                ],
            )
            .privileged(),
        );
        becomes_available.push(ManagerId::Brew);
        becomes_available.push(ManagerId::BrewCask);
    }
    if !probe.mise {
        cmds.push(Cmd::new("brew", ["install", "mise"]));
        becomes_available.push(ManagerId::Mise);
    }
    (cmds, becomes_available)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CliOverrides, ConfigFile, resolve};
    use crate::platform::test_platforms::*;

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
            ..Default::default()
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
        let plan = build(&only(&["core"]), &PLAIN, &snapshot(&[ManagerId::Brew]));

        let brew = installs_for(&plan, ManagerId::Brew).expect("expected a brew install");
        assert!(brew.contains(&"ripgrep".to_string()), "got {brew:?}");
        assert!(plan.missing().contains(&"ripgrep".to_string()));
    }

    #[test]
    fn an_already_installed_package_produces_no_action() {
        // The idempotency guarantee: a converged machine plans nothing.
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed
            .insert(ManagerId::Brew, ids_in("core", ManagerId::Brew));
        snap.taps.insert("powertmux/powertmux".to_string());
        snap.trusted_taps
            .insert("/x/homebrew-powertmux".to_string());
        // The two dnf-only entries resolve through their binaries.
        snap.binaries
            .extend(["nc".to_string(), "toolbox".to_string()]);

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            plan.is_empty(),
            "expected no actions, got {:?}",
            plan.actions
        );
        assert!(plan.satisfied().contains(&"ripgrep".to_string()));
    }

    #[test]
    fn a_disabled_bundle_contributes_nothing() {
        let plan = build(&only(&[]), &PLAIN, &snapshot(&[ManagerId::Brew]));

        assert!(plan.is_empty(), "got {:?}", plan.actions);
        assert!(plan.packages.is_empty());
    }

    #[test]
    fn a_bundle_barred_on_this_platform_is_skipped_not_installed() {
        for platform in [UNDER_WSL, SERVER, CONTAINER] {
            let plan = build(
                &only(&["desktop", "fonts"]),
                &platform,
                &snapshot(ManagerId::ALL),
            );

            assert!(plan.is_empty(), "{platform:?}: got {:?}", plan.actions);
            assert!(
                plan.skipped.iter().any(|s| s.name == "desktop")
                    && plan.skipped.iter().any(|s| s.name == "fonts"),
                "the skip should be reported, got {:?}",
                plan.skipped
            );
        }
    }

    #[test]
    fn a_skipped_bundle_says_why() {
        let plan = build(&only(&["desktop"]), &CONTAINER, &snapshot(ManagerId::ALL));

        assert!(
            plan.skipped[0].reason.contains("container"),
            "{:?}",
            plan.skipped
        );
    }

    #[test]
    fn an_extra_naming_an_unusable_manager_is_reported_unavailable() {
        let plan = build(
            &cfg(&format!(
                "{}\n[extra]\ndnf = [\"some-kernel-tool\"]\n",
                none_toml()
            )),
            &ATOMIC,
            &snapshot(&[ManagerId::Brew]),
        );

        let unavailable = plan.unavailable();
        assert_eq!(unavailable.len(), 1);
        assert_eq!(unavailable[0].package, "some-kernel-tool");
        assert!(unavailable[0].reason.contains("dnf"));
    }

    #[test]
    fn a_dnf_only_catalog_package_without_its_binary_is_unavailable_on_atomic() {
        // toolbox: ships with atomic images, so normally satisfied by PATH.
        // Without it there is no user-space provider at all.
        let plan = build(&only(&["core"]), &ATOMIC, &snapshot(&[ManagerId::Brew]));

        let toolbox = plan.packages.iter().find(|p| p.name == "toolbox").unwrap();
        assert!(
            matches!(&toolbox.state, State::Unavailable(r) if r.contains("atomic")),
            "got {:?}",
            toolbox.state
        );
    }

    #[test]
    fn no_dnf_action_is_ever_planned_on_an_atomic_host() {
        let all: Vec<&str> = BUNDLES.iter().map(|b| b.name).collect();
        let plan = build(&only(&all), &ATOMIC, &snapshot(ManagerId::ALL));

        for action in &plan.actions {
            assert_ne!(
                action.manager(),
                ManagerId::Dnf,
                "planned a dnf action on atomic"
            );
        }
    }

    #[test]
    fn a_tapped_package_is_preceded_by_its_tap_and_trust() {
        let plan = build(&only(&["core"]), &PLAIN, &snapshot(&[ManagerId::Brew]));

        let at = |f: fn(&Action) -> bool| plan.actions.iter().position(f);
        let tap = at(|a| matches!(a, Action::Tap { tap, .. } if tap == "powertmux/powertmux"))
            .expect("tap");
        let trust = at(|a| matches!(a, Action::Trust { .. })).expect("trust");
        let install = at(|a| {
            matches!(
                a,
                Action::Install {
                    manager: ManagerId::Brew,
                    ..
                }
            )
        })
        .expect("install");

        assert!(tap < trust, "tap before trust");
        assert!(trust < install, "trust before install");
    }

    #[test]
    fn an_existing_trusted_tap_is_neither_added_nor_trusted_again() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.taps.insert("powertmux/powertmux".to_string());
        snap.trusted_taps
            .insert("/home/x/Library/Taps/powertmux/homebrew-powertmux".to_string());

        let plan = build(&only(&["core"]), &PLAIN, &snap);

        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::Tap { .. } | Action::Trust { .. })),
            "got {:?}",
            plan.actions
        );
    }

    #[test]
    fn installs_are_grouped_into_one_command_per_manager() {
        let plan = build(
            &only(&["core", "security", "go"]),
            &PLAIN,
            &snapshot(&[ManagerId::Brew, ManagerId::Mise]),
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

        let mise = installs_for(&plan, ManagerId::Mise).expect("a mise install");
        assert_eq!(mise, vec!["go@latest".to_string()]);
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
    fn the_upgrade_option_upgrades_installed_packages_and_extras() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        snap.installed.insert(
            ManagerId::Brew,
            HashSet::from(["ripgrep".to_string(), "jless".to_string()]),
        );
        let mut toml = none_toml().replace("core = false", "core = true");
        toml.push_str("\n[options]\nupgrade = true\n[extra]\nbrew = [\"jless\"]\n");

        let plan = build(&cfg(&toml), &PLAIN, &snap);

        let upgraded = plan
            .actions
            .iter()
            .find_map(|a| match a {
                Action::Upgrade { packages, .. } => Some(packages.clone()),
                _ => None,
            })
            .expect("an upgrade");
        assert!(upgraded.contains(&"ripgrep".to_string()), "{upgraded:?}");
        assert!(upgraded.contains(&"jless".to_string()), "{upgraded:?}");
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
        assert_eq!(plan.packages[0].bundle, "extra");
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
        let mut snap = snapshot(&[ManagerId::Brew, ManagerId::Flatpak]);
        snap.installed.insert(
            ManagerId::Flatpak,
            HashSet::from([
                "com.spotify.Client".to_string(),
                "org.remmina.Remmina".to_string(),
            ]),
        );
        snap.binaries.insert("xdotool".to_string());

        let plan = build(&only(&["desktop"]), &PLAIN, &snap);

        assert!(plan.is_empty(), "got {:?}", plan.actions);
    }

    #[test]
    fn a_flatpak_install_adds_the_user_remote_first_when_missing() {
        let plan = build(
            &only(&["desktop"]),
            &PLAIN,
            &snapshot(&[ManagerId::Flatpak]),
        );

        let remote = plan
            .actions
            .iter()
            .position(|a| matches!(a, Action::AddRemote { .. }))
            .expect("a remote-add action");
        let install = plan
            .actions
            .iter()
            .position(|a| matches!(a, Action::Install { .. }))
            .expect("an install");
        assert!(remote < install);
        assert!(
            plan.actions[remote]
                .to_cmd()
                .to_shell()
                .starts_with("flatpak remote-add --user")
        );
    }

    #[test]
    fn an_existing_user_remote_is_not_added_again() {
        let mut snap = snapshot(&[ManagerId::Flatpak]);
        snap.remotes.insert("flathub".to_string());

        let plan = build(&only(&["desktop"]), &PLAIN, &snap);

        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::AddRemote { .. })),
            "got {:?}",
            plan.actions
        );
    }

    #[test]
    fn no_remote_is_added_when_nothing_needs_installing_from_it() {
        let plan = build(
            &only(&["core"]),
            &PLAIN,
            &snapshot(&[ManagerId::Brew, ManagerId::Flatpak]),
        );

        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::AddRemote { .. })),
            "got {:?}",
            plan.actions
        );
    }

    #[test]
    fn only_the_selected_prompt_is_installed() {
        let mut toml = none_toml().replace("prompt = false", "prompt = true");
        toml.push_str("\n[shell]\nprompt = \"oh-my-posh\"\n");

        let plan = build(&cfg(&toml), &PLAIN, &snapshot(&[ManagerId::Brew]));

        assert_eq!(
            installs_for(&plan, ManagerId::Brew).unwrap(),
            vec!["oh-my-posh".to_string()]
        );
        assert!(
            !plan.actions.iter().any(|a| matches!(a, Action::Tap { .. })),
            "powerbash's tap must not be added when it is not selected"
        );
    }

    #[test]
    fn selecting_powerbash_taps_its_repository() {
        let mut toml = none_toml().replace("prompt = false", "prompt = true");
        toml.push_str("\n[shell]\nprompt = \"powerbash\"\n");

        let plan = build(&cfg(&toml), &PLAIN, &snapshot(&[ManagerId::Brew]));

        assert!(
            plan.actions
                .iter()
                .any(|a| matches!(a, Action::Tap { tap, .. } if tap == "powerbash/powerbash")),
            "got {:?}",
            plan.actions
        );
    }

    #[test]
    fn a_mise_toolchain_is_satisfied_only_by_mise_not_by_a_binary_on_path() {
        let mut snap = snapshot(&[ManagerId::Brew, ManagerId::Mise]);
        snap.binaries.insert("go".to_string());

        let plan = build(&only(&["go"]), &PLAIN, &snap);

        assert_eq!(
            installs_for(&plan, ManagerId::Mise).unwrap(),
            vec!["go@latest".to_string()],
            "the OS `go` is not the managed one"
        );
    }

    #[test]
    fn a_mise_toolchain_present_in_the_global_config_is_satisfied() {
        let mut snap = snapshot(&[ManagerId::Brew, ManagerId::Mise]);
        snap.installed.insert(
            ManagerId::Mise,
            HashSet::from(["java@corretto-21".to_string()]),
        );

        let plan = build(&only(&["java"]), &PLAIN, &snap);

        let java = plan.packages.iter().find(|p| p.name == "java").unwrap();
        assert_eq!(java.state, State::Installed);
        assert!(
            !installs_for(&plan, ManagerId::Mise)
                .unwrap()
                .iter()
                .any(|p| p.starts_with("java@"))
        );
    }

    #[test]
    fn a_binary_on_path_satisfies_an_ordinary_package_without_upgrading_it() {
        let mut snap = snapshot(&[ManagerId::Brew]);
        for p in BUNDLES.iter().find(|b| b.name == "core").unwrap().packages {
            if let Some(bin) = p.binary {
                snap.binaries.insert(bin.to_string());
            }
        }
        let mut toml = none_toml().replace("core = false", "core = true");
        toml.push_str("\n[options]\nupgrade = true\n");

        let plan = build(&cfg(&toml), &PLAIN, &snap);

        assert!(plan.is_empty(), "got {:?}", plan.actions);
        let vim = plan.packages.iter().find(|p| p.name == "vim").unwrap();
        assert_eq!(vim.state, State::OnPath);
    }

    #[test]
    fn actions_render_their_managers_commands() {
        assert_eq!(
            Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into(), "fd".into()],
            }
            .to_cmd()
            .to_shell(),
            "brew install bat fd"
        );
        assert_eq!(
            Action::Tap {
                manager: ManagerId::Brew,
                tap: "powertmux/powertmux".into(),
            }
            .to_cmd()
            .to_shell(),
            "brew tap powertmux/powertmux"
        );
        assert_eq!(
            Action::Trust {
                manager: ManagerId::Brew,
                tap: "powertmux/powertmux".into(),
            }
            .to_cmd()
            .to_shell(),
            "brew trust --tap powertmux/powertmux"
        );
        assert_eq!(
            Action::Upgrade {
                manager: ManagerId::Npm,
                packages: vec!["pyright".into()],
            }
            .to_cmd()
            .to_shell(),
            "npm update -g pyright"
        );
        assert!(
            Action::AddRemote {
                manager: ManagerId::Flatpak
            }
            .to_cmd()
            .to_shell()
            .contains("flathub")
        );
    }

    #[test]
    fn a_plan_carrying_only_a_dotfiles_or_bootstrap_step_is_not_empty() {
        let plan = ActionPlan {
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };
        assert!(!plan.is_empty());

        let plan = ActionPlan {
            bootstrap: vec![Cmd::new("brew", ["install", "mise"])],
            ..Default::default()
        };
        assert!(!plan.is_empty());
    }

    #[test]
    fn commands_run_bootstrap_then_actions_then_dotfiles() {
        let plan = ActionPlan {
            bootstrap: vec![Cmd::new("brew", ["install", "mise"])],
            actions: vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into()],
            }],
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        let cmds: Vec<String> = plan.commands().iter().map(Cmd::to_shell).collect();

        assert_eq!(
            cmds,
            vec!["brew install mise", "brew install bat", "chezmoi apply"]
        );
    }

    // --- bootstrap ---------------------------------------------------------

    #[test]
    fn a_host_with_both_managers_needs_no_bootstrap() {
        let probe = Probe {
            brew: true,
            mise: true,
            sudo: true,
        };
        for platform in [ATOMIC, PLAIN, SERVER, UNDER_WSL, CONTAINER] {
            let (cmds, managers) = bootstrap(&platform, probe);
            assert!(cmds.is_empty(), "{platform:?}: {cmds:?}");
            assert!(managers.is_empty());
        }
    }

    #[test]
    fn a_fresh_fedora_installs_prerequisites_then_homebrew_then_mise() {
        let probe = Probe {
            brew: false,
            mise: false,
            sudo: true,
        };
        for platform in [PLAIN, SERVER, UNDER_WSL, CONTAINER] {
            let (cmds, managers) = bootstrap(&platform, probe);
            let shells: Vec<String> = cmds.iter().map(Cmd::to_shell).collect();

            assert_eq!(shells.len(), 3, "{platform:?}: {shells:?}");
            assert!(shells[0].starts_with("sudo dnf install -y"), "{shells:?}");
            assert!(shells[0].contains("git") && shells[0].contains("gcc"));
            assert!(shells[1].contains(BREW_INSTALLER), "{shells:?}");
            assert!(shells[1].contains("NONINTERACTIVE=1"), "{shells:?}");
            assert_eq!(shells[2], "brew install mise");
            assert_eq!(
                managers,
                vec![ManagerId::Brew, ManagerId::BrewCask, ManagerId::Mise]
            );
        }
    }

    #[test]
    fn an_atomic_host_without_homebrew_skips_the_dnf_prerequisites() {
        let probe = Probe {
            brew: false,
            mise: false,
            sudo: true,
        };

        let (cmds, _) = bootstrap(&ATOMIC, probe);
        let shells: Vec<String> = cmds.iter().map(Cmd::to_shell).collect();

        assert!(!shells.iter().any(|s| s.contains("dnf")), "{shells:?}");
        assert!(shells[0].contains(BREW_INSTALLER));
    }

    #[test]
    fn without_sudo_the_prerequisites_cannot_be_planned() {
        let probe = Probe {
            brew: false,
            mise: true,
            sudo: false,
        };

        let (cmds, _) = bootstrap(&PLAIN, probe);

        assert!(!cmds.iter().any(|c| c.to_shell().contains("dnf")));
    }

    #[test]
    fn the_homebrew_installer_step_keeps_the_terminal() {
        // It calls sudo itself to create /home/linuxbrew.
        let (cmds, _) = bootstrap(
            &PLAIN,
            Probe {
                brew: false,
                mise: true,
                sudo: true,
            },
        );

        assert!(cmds.iter().all(|c| c.privileged), "{cmds:?}");
    }

    #[test]
    fn mise_alone_comes_from_homebrew() {
        let (cmds, managers) = bootstrap(
            &ATOMIC,
            Probe {
                brew: true,
                mise: false,
                sudo: false,
            },
        );

        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].to_shell(), "brew install mise");
        assert!(!cmds[0].privileged);
        assert_eq!(managers, vec![ManagerId::Mise]);
    }
}
