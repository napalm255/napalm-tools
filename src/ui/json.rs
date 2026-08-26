//! Machine-readable views.
//!
//! Deliberately separate structs rather than `Serialize` on the domain types.
//! Once anything scripts against this output it is an interface, and an
//! interface should not change because someone renamed a Rust field.

use serde::Serialize;

use crate::bundles::BUNDLES;
use crate::config::Resolved;
use crate::execute::RunReport;
use crate::plan::{Action, ActionPlan, State};
use crate::platform::Platform;
use crate::report;

/// One planned step.
#[derive(Debug, Serialize)]
pub struct ActionView {
    /// `bootstrap`, `remote`, `tap`, `trust`, `install`, `upgrade` or
    /// `dotfiles`.
    pub kind: &'static str,
    /// The manager it runs against, if any.
    pub manager: Option<String>,
    /// Packages or taps involved.
    pub packages: Vec<String>,
    /// The command as it would be typed.
    pub command: String,
    /// Whether this step may need elevated privileges, and so a password.
    pub privileged: bool,
}

/// One wanted package.
#[derive(Debug, Serialize)]
pub struct PackageView {
    /// Bundle it came from, or `extra`.
    pub bundle: String,
    /// Package name.
    pub name: String,
    /// The manager that would supply it, if any.
    pub manager: Option<String>,
    /// The identifier within that manager.
    pub id: Option<String>,
    /// `installed`, `on-path`, `missing` or `unavailable`.
    pub state: &'static str,
    /// Why it is unavailable, when it is.
    pub reason: Option<String>,
}

/// A package that cannot be provisioned here.
#[derive(Debug, Serialize)]
pub struct UnavailableView {
    /// Package name.
    pub package: String,
    /// Bundle it came from, or `extra`.
    pub source: String,
    /// Why not.
    pub reason: String,
}

/// A bundle that does not apply to this host.
#[derive(Debug, Serialize)]
pub struct SkippedView {
    /// Bundle name.
    pub name: String,
    /// Why it was skipped.
    pub reason: String,
}

/// A whole plan.
#[derive(Debug, Serialize)]
pub struct PlanView {
    /// Whether this was a simulation.
    pub dry_run: bool,
    /// Where the host was detected to be.
    pub platform: PlatformView,
    /// Steps that would run, in order.
    pub actions: Vec<ActionView>,
    /// Every wanted package and its state.
    pub packages: Vec<PackageView>,
    /// Packages with no provider here.
    pub unavailable: Vec<UnavailableView>,
    /// Bundles enabled but not applicable.
    pub skipped: Vec<SkippedView>,
    /// Names of packages already present.
    pub satisfied: Vec<String>,
}

/// One bundle's state on this host.
#[derive(Debug, Serialize)]
pub struct BundleView {
    /// Bundle name, which `--skip` and `--only` take.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Whether it is on after resolution.
    pub enabled: bool,
    /// Whether it applies to this platform.
    pub applicable: bool,
    /// Why it does not apply, when it does not.
    pub reason: Option<String>,
    /// The packages it wants here.
    pub packages: Vec<CatalogPackageView>,
}

/// One catalog package and its providers.
#[derive(Debug, Serialize)]
pub struct CatalogPackageView {
    /// Package name.
    pub name: String,
    /// The executable it provides, if any.
    pub binary: Option<String>,
    /// Ways to obtain it, most preferred first.
    pub providers: Vec<ProviderView>,
}

/// One way of obtaining a package.
#[derive(Debug, Serialize)]
pub struct ProviderView {
    /// Manager name.
    pub manager: String,
    /// Identifier within the manager.
    pub id: String,
    /// Homebrew tap, if one is needed.
    pub tap: Option<String>,
    /// Whether it is usable on this platform.
    pub applicable: bool,
}

/// The catalog.
#[derive(Debug, Serialize)]
pub struct BundlesView {
    /// The selected prompt, which decides the `prompt` bundle's contents.
    pub prompt: String,
    /// Every bundle.
    pub bundles: Vec<BundleView>,
}

/// The detected platform.
#[derive(Debug, Serialize)]
pub struct PlatformView {
    /// Fedora or a derivative.
    pub fedora_family: bool,
    /// Booted from an ostree commit.
    pub atomic: bool,
    /// Running under WSL.
    pub wsl: bool,
    /// Running inside a container.
    pub container: bool,
    /// A desktop session is installed.
    pub graphical: bool,
}

/// Resolved configuration.
#[derive(Debug, Serialize)]
pub struct ConfigView {
    /// Where the host was detected to be.
    pub platform: PlatformView,
    /// The shell prompt.
    pub prompt: String,
    /// Upgrade already-installed packages.
    pub upgrade: bool,
    /// Fail on an unprovisionable package.
    pub strict: bool,
    /// Bundle states by name.
    pub bundles: std::collections::BTreeMap<String, bool>,
    /// Extra packages by manager.
    pub extra: std::collections::BTreeMap<String, Vec<String>>,
}

/// What a run did.
#[derive(Debug, Serialize)]
pub struct ReportView {
    /// Each step that ran.
    pub steps: Vec<StepView>,
    /// Seconds the run took.
    pub duration_secs: f64,
    /// Caveats surfaced by the managers.
    pub caveats: Vec<CaveatView>,
    /// Warnings surfaced by the managers.
    pub warnings: Vec<String>,
}

/// One executed step.
#[derive(Debug, Serialize)]
pub struct StepView {
    /// The command that ran.
    pub command: String,
    /// Whether it succeeded.
    pub success: bool,
    /// Seconds it took.
    pub duration_secs: f64,
}

/// A caveat block.
#[derive(Debug, Serialize)]
pub struct CaveatView {
    /// The command that produced it.
    pub source: String,
    /// Its text.
    pub text: String,
}

fn platform_view(platform: &Platform) -> PlatformView {
    PlatformView {
        fedora_family: platform.fedora_family,
        atomic: platform.atomic,
        wsl: platform.wsl,
        container: platform.container,
        graphical: platform.graphical,
    }
}

/// Build the view of a plan.
pub fn plan_view(plan: &ActionPlan, platform: &Platform, dry_run: bool) -> PlanView {
    let mut actions: Vec<ActionView> = plan
        .bootstrap
        .iter()
        .map(|cmd| ActionView {
            kind: "bootstrap",
            manager: None,
            packages: Vec::new(),
            command: cmd.to_shell(),
            privileged: cmd.privileged,
        })
        .collect();

    actions.extend(plan.actions.iter().map(|a| {
        let (kind, packages) = match a {
            Action::AddRemote { .. } => ("remote", Vec::new()),
            Action::Tap { tap, .. } => ("tap", vec![tap.clone()]),
            Action::Trust { tap, .. } => ("trust", vec![tap.clone()]),
            Action::Install { packages, .. } => ("install", packages.clone()),
            Action::Upgrade { packages, .. } => ("upgrade", packages.clone()),
        };
        let cmd = a.to_cmd();
        ActionView {
            kind,
            manager: Some(a.manager().as_str().to_string()),
            packages,
            command: cmd.to_shell(),
            privileged: cmd.privileged,
        }
    }));

    // Dotfiles are part of the plan, so they are part of the document.
    actions.extend(plan.dotfiles.iter().map(|cmd| ActionView {
        kind: "dotfiles",
        manager: None,
        packages: Vec::new(),
        command: cmd.to_shell(),
        privileged: cmd.privileged,
    }));

    PlanView {
        dry_run,
        platform: platform_view(platform),
        actions,
        packages: plan
            .packages
            .iter()
            .map(|p| PackageView {
                bundle: p.bundle.clone(),
                name: p.name.clone(),
                manager: p.provider.as_ref().map(|(m, _)| m.as_str().to_string()),
                id: p.provider.as_ref().map(|(_, id)| id.clone()),
                state: match p.state {
                    State::Installed => "installed",
                    State::OnPath => "on-path",
                    State::Missing => "missing",
                    State::Unavailable(_) => "unavailable",
                },
                reason: match &p.state {
                    State::Unavailable(r) => Some(r.clone()),
                    _ => None,
                },
            })
            .collect(),
        unavailable: plan
            .unavailable()
            .into_iter()
            .map(|u| UnavailableView {
                package: u.package,
                source: u.source,
                reason: u.reason,
            })
            .collect(),
        skipped: plan
            .skipped
            .iter()
            .map(|s| SkippedView {
                name: s.name.clone(),
                reason: s.reason.clone(),
            })
            .collect(),
        satisfied: plan.satisfied(),
    }
}

/// Build the view of the catalog.
pub fn bundles_view(resolved: &Resolved, platform: &Platform) -> BundlesView {
    BundlesView {
        prompt: resolved.prompt.clone(),
        bundles: BUNDLES
            .iter()
            .map(|b| BundleView {
                name: b.name.to_string(),
                description: b.description.to_string(),
                enabled: resolved.bundle_enabled(b.name),
                applicable: b.platforms.matches(platform),
                reason: b.platforms.rejection(platform).map(str::to_string),
                packages: b
                    .wanted(&resolved.prompt)
                    .map(|p| CatalogPackageView {
                        name: p.name.to_string(),
                        binary: p.binary.map(str::to_string),
                        providers: p
                            .providers
                            .iter()
                            .map(|pr| ProviderView {
                                manager: pr.manager.as_str().to_string(),
                                id: pr.id.to_string(),
                                tap: pr.tap.map(str::to_string),
                                applicable: pr.platforms.matches(platform),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// Build the view of resolved configuration.
pub fn config_view(resolved: &Resolved, platform: &Platform) -> ConfigView {
    ConfigView {
        platform: platform_view(platform),
        prompt: resolved.prompt.clone(),
        upgrade: resolved.upgrade,
        strict: resolved.strict,
        bundles: resolved.bundles.clone(),
        extra: resolved
            .extra
            .iter()
            .map(|(m, p)| (m.as_str().to_string(), p.clone()))
            .collect(),
    }
}

/// Build the view of a completed run.
pub fn report_view(report: &RunReport) -> ReportView {
    ReportView {
        steps: report
            .steps
            .iter()
            .map(|s| StepView {
                command: s.command.clone(),
                success: s.success,
                duration_secs: s.duration.as_secs_f64(),
            })
            .collect(),
        duration_secs: report.total.as_secs_f64(),
        caveats: report
            .findings
            .caveats
            .iter()
            .map(|c| CaveatView {
                source: c.source.clone(),
                text: c.lines.join("\n"),
            })
            .collect(),
        warnings: report.findings.warnings.clone(),
    }
}

/// Status: the plan's packages tallied per bundle.
#[derive(Debug, Serialize)]
pub struct StatusView {
    /// Where the host was detected to be.
    pub platform: PlatformView,
    /// Per-bundle counts.
    pub bundles: Vec<BundleTallyView>,
    /// Bundles enabled but not applicable.
    pub skipped: Vec<SkippedView>,
    /// Every wanted package and its state.
    pub packages: Vec<PackageView>,
    /// Totals across every bundle.
    pub totals: TallyView,
}

/// Counts for one bundle.
#[derive(Debug, Serialize)]
pub struct BundleTallyView {
    /// Bundle name, or `extra`.
    pub name: String,
    /// Its counts.
    #[serde(flatten)]
    pub tally: TallyView,
}

/// Package counts.
#[derive(Debug, Serialize, Default)]
pub struct TallyView {
    /// Installed or on `PATH`.
    pub present: usize,
    /// An action would install it.
    pub missing: usize,
    /// No provider here.
    pub unavailable: usize,
}

/// Build the status view.
pub fn status_view(plan: &ActionPlan, platform: &Platform) -> StatusView {
    let mut totals = TallyView::default();
    let bundles = report::tally(plan)
        .into_iter()
        .map(|(name, t)| {
            totals.present += t.present;
            totals.missing += t.missing;
            totals.unavailable += t.unavailable;
            BundleTallyView {
                name,
                tally: TallyView {
                    present: t.present,
                    missing: t.missing,
                    unavailable: t.unavailable,
                },
            }
        })
        .collect();
    let full = plan_view(plan, platform, true);
    StatusView {
        platform: full.platform,
        bundles,
        skipped: full.skipped,
        packages: full.packages,
        totals,
    }
}

/// Serialise a view as pretty-printed JSON with a trailing newline.
pub fn to_string<T: Serialize>(value: &T) -> String {
    match serde_json::to_string_pretty(value) {
        Ok(mut s) => {
            s.push('\n');
            s
        }
        // Views are plain data; failure here is not something a caller can act
        // on, so emit valid JSON describing the problem rather than panicking.
        Err(e) => format!("{{\"error\":\"failed to serialise output: {e}\"}}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CliOverrides, ConfigFile, resolve};
    use crate::managers::{Cmd, ManagerId};
    use crate::plan::{PackageState, SkippedBundle};
    use crate::platform::test_platforms::*;

    fn sample_plan() -> ActionPlan {
        ActionPlan {
            bootstrap: vec![Cmd::new("brew", ["install", "mise"])],
            actions: vec![
                Action::Tap {
                    manager: ManagerId::Brew,
                    tap: "powertmux/powertmux".into(),
                },
                Action::Install {
                    manager: ManagerId::Brew,
                    packages: vec!["nmap".into(), "inetutils".into()],
                },
            ],
            packages: vec![
                PackageState {
                    bundle: "core".into(),
                    name: "ripgrep".into(),
                    provider: Some((ManagerId::Brew, "ripgrep".into())),
                    state: State::Installed,
                },
                PackageState {
                    bundle: "desktop".into(),
                    name: "xdotool".into(),
                    provider: None,
                    state: State::Unavailable("no user-space provider on an atomic host".into()),
                },
            ],
            skipped: vec![SkippedBundle {
                name: "fonts".into(),
                reason: "needs a desktop session; this is WSL".into(),
            }],
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
        }
    }

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("must parse")
    }

    fn keys(v: &serde_json::Value) -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn the_plan_document_keys_are_stable() {
        // This is an interface. A rename should fail here, not in a script.
        let v = parse(&to_string(&plan_view(&sample_plan(), &ATOMIC, true)));

        assert_eq!(
            keys(&v),
            vec![
                "actions",
                "dry_run",
                "packages",
                "platform",
                "satisfied",
                "skipped",
                "unavailable"
            ]
        );
    }

    #[test]
    fn actions_run_bootstrap_first_and_dotfiles_last_with_named_kinds() {
        let v = parse(&to_string(&plan_view(&sample_plan(), &ATOMIC, true)));
        let kinds: Vec<&str> = v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["kind"].as_str().unwrap())
            .collect();

        assert_eq!(kinds, vec!["bootstrap", "tap", "install", "dotfiles"]);
        assert_eq!(v["actions"][2]["manager"], "brew");
        assert_eq!(v["actions"][2]["command"], "brew install nmap inetutils");
    }

    #[test]
    fn packages_carry_their_state_and_reason() {
        let v = parse(&to_string(&plan_view(&sample_plan(), &ATOMIC, true)));

        assert_eq!(v["packages"][0]["state"], "installed");
        assert_eq!(v["packages"][0]["manager"], "brew");
        assert_eq!(v["packages"][1]["state"], "unavailable");
        assert!(
            v["packages"][1]["reason"]
                .as_str()
                .unwrap()
                .contains("atomic")
        );
        assert_eq!(v["unavailable"][0]["package"], "xdotool");
        assert_eq!(v["satisfied"][0], "ripgrep");
    }

    #[test]
    fn the_bundles_document_lists_every_bundle_with_its_packages() {
        let resolved =
            resolve(&ConfigFile::default(), "testhost", &CliOverrides::default()).unwrap();

        let v = parse(&to_string(&bundles_view(&resolved, &UNDER_WSL)));

        assert_eq!(v["prompt"], "starship");
        assert_eq!(v["bundles"].as_array().unwrap().len(), BUNDLES.len());
        assert_eq!(v["bundles"][0]["name"], "core");
        assert_eq!(v["bundles"][0]["enabled"], true);
        assert_eq!(v["bundles"][0]["packages"][0]["name"], "ripgrep");
        assert_eq!(
            v["bundles"][0]["packages"][0]["providers"][0]["manager"],
            "brew"
        );
        let desktop = v["bundles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == "desktop")
            .unwrap();
        assert_eq!(desktop["applicable"], false);
        assert!(desktop["reason"].as_str().unwrap().contains("WSL"));
    }

    #[test]
    fn the_config_document_reports_the_platform_and_prompt() {
        let resolved =
            resolve(&ConfigFile::default(), "testhost", &CliOverrides::default()).unwrap();

        let v = parse(&to_string(&config_view(&resolved, &CONTAINER)));

        assert_eq!(v["platform"]["container"], true);
        assert_eq!(v["platform"]["graphical"], false);
        assert_eq!(v["prompt"], "starship");
        assert_eq!(v["upgrade"], false);
    }

    #[test]
    fn the_report_document_keys_are_stable() {
        let v = parse(&to_string(&report_view(&RunReport::default())));

        assert_eq!(
            keys(&v),
            vec!["caveats", "duration_secs", "steps", "warnings"]
        );
    }

    #[test]
    fn the_status_document_tallies_per_bundle_and_in_total() {
        let v = parse(&to_string(&status_view(&sample_plan(), &PLAIN)));

        assert_eq!(
            keys(&v),
            vec!["bundles", "packages", "platform", "skipped", "totals"]
        );
        assert_eq!(v["bundles"][0]["name"], "core");
        assert_eq!(v["bundles"][0]["present"], 1);
        assert_eq!(v["totals"]["present"], 1);
        assert_eq!(v["totals"]["unavailable"], 1);
        assert_eq!(v["skipped"][0]["name"], "fonts");
    }

    #[test]
    fn actions_report_whether_they_need_a_password() {
        let plan = ActionPlan {
            bootstrap: vec![Cmd::new("sudo", ["dnf", "install", "-y", "git"]).privileged()],
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        let v = parse(&to_string(&plan_view(&plan, &PLAIN, true)));

        assert_eq!(v["actions"][0]["privileged"], true);
        assert_eq!(v["actions"][1]["privileged"], false);
    }
}
