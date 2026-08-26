//! Machine-readable views.
//!
//! Deliberately separate structs rather than `Serialize` on the domain types.
//! Once anything scripts against this output it is an interface, and an
//! interface should not change because someone renamed a Rust field.

use serde::Serialize;

use crate::bundles::BUNDLES;
use crate::config::Resolved;
use crate::execute::RunReport;
use crate::plan::{Action, ActionPlan};
use crate::platform::Platform;

/// One planned step.
#[derive(Debug, Serialize)]
pub struct ActionView {
    /// `tap`, `install`, `upgrade` or `dotfiles`.
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
    /// Steps that would run, in order.
    pub actions: Vec<ActionView>,
    /// Packages with no provider here.
    pub unavailable: Vec<UnavailableView>,
    /// Bundles enabled but not applicable.
    pub skipped: Vec<SkippedView>,
    /// Packages already present.
    pub satisfied: Vec<String>,
}

/// One bundle's state on this host.
#[derive(Debug, Serialize)]
pub struct BundleView {
    /// Bundle name, which is also its CLI flag.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Whether it is on after resolution.
    pub enabled: bool,
    /// Whether it applies to this platform.
    pub applicable: bool,
    /// How many packages it contains.
    pub packages: usize,
}

/// The catalog.
#[derive(Debug, Serialize)]
pub struct BundlesView {
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
}

/// Resolved configuration.
#[derive(Debug, Serialize)]
pub struct ConfigView {
    /// Where the host was detected to be.
    pub platform: PlatformView,
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

/// Build the view of a plan.
pub fn plan_view(plan: &ActionPlan, dry_run: bool) -> PlanView {
    let mut actions: Vec<ActionView> = plan
        .actions
        .iter()
        .map(|a| {
            let (kind, packages) = match a {
                Action::Tap { tap, .. } => ("tap", vec![tap.clone()]),
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
        })
        .collect();

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
        actions,
        unavailable: plan
            .unavailable
            .iter()
            .map(|u| UnavailableView {
                package: u.package.clone(),
                source: u.source.clone(),
                reason: u.reason.clone(),
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
        satisfied: plan.satisfied.clone(),
    }
}

/// Build the view of the catalog.
pub fn bundles_view(resolved: &Resolved, platform: &Platform) -> BundlesView {
    BundlesView {
        bundles: BUNDLES
            .iter()
            .map(|b| BundleView {
                name: b.name.to_string(),
                description: b.description.to_string(),
                enabled: resolved.bundle_enabled(b.name),
                applicable: b.platforms.matches(platform),
                packages: b.packages.len(),
            })
            .collect(),
    }
}

/// Build the view of resolved configuration.
pub fn config_view(resolved: &Resolved, platform: &Platform) -> ConfigView {
    ConfigView {
        platform: PlatformView {
            fedora_family: platform.fedora_family,
            atomic: platform.atomic,
            wsl: platform.wsl,
        },
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
    use crate::managers::ManagerId;
    use crate::plan::{SkippedBundle, Unavailable};

    const ATOMIC: Platform = Platform {
        fedora_family: true,
        atomic: true,
        wsl: false,
    };

    fn sample_plan() -> ActionPlan {
        ActionPlan {
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
            unavailable: vec![Unavailable {
                package: "xdotool".into(),
                source: "desktop".into(),
                reason: "no user-space provider on an atomic host".into(),
            }],
            skipped: vec![SkippedBundle {
                name: "fonts".into(),
                reason: "not applicable to this platform".into(),
            }],
            satisfied: vec!["ripgrep".into()],
            dotfiles: vec![crate::managers::Cmd::new("chezmoi", ["apply"])],
        }
    }

    fn keys(json: &str) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn a_plan_serialises_to_valid_json() {
        let json = to_string(&plan_view(&sample_plan(), true));

        serde_json::from_str::<serde_json::Value>(&json).expect("must parse");
    }

    #[test]
    fn the_plan_document_keys_are_stable() {
        // This is an interface. A rename should fail here, not in a script.
        let json = to_string(&plan_view(&sample_plan(), true));

        assert_eq!(
            keys(&json),
            vec!["actions", "dry_run", "satisfied", "skipped", "unavailable"]
        );
    }

    #[test]
    fn every_action_carries_the_command_it_would_run() {
        let json = to_string(&plan_view(&sample_plan(), true));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        let commands: Vec<&str> = v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["command"].as_str().unwrap())
            .collect();

        assert!(
            commands.contains(&"brew tap powertmux/powertmux"),
            "got {commands:?}"
        );
        assert!(
            commands.contains(&"brew install nmap inetutils"),
            "got {commands:?}"
        );
    }

    #[test]
    fn dotfile_steps_appear_as_actions() {
        let v: serde_json::Value =
            serde_json::from_str(&to_string(&plan_view(&sample_plan(), true))).unwrap();
        let kinds: Vec<&str> = v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["kind"].as_str().unwrap())
            .collect();

        assert!(kinds.contains(&"dotfiles"), "got {kinds:?}");
    }

    #[test]
    fn action_kinds_are_named_not_numbered() {
        let v: serde_json::Value =
            serde_json::from_str(&to_string(&plan_view(&sample_plan(), true))).unwrap();

        assert_eq!(v["actions"][0]["kind"], "tap");
        assert_eq!(v["actions"][1]["kind"], "install");
        assert_eq!(v["actions"][1]["manager"], "brew");
    }

    #[test]
    fn unavailable_packages_keep_their_reason() {
        let v: serde_json::Value =
            serde_json::from_str(&to_string(&plan_view(&sample_plan(), true))).unwrap();

        assert_eq!(v["unavailable"][0]["package"], "xdotool");
        assert!(
            v["unavailable"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("atomic")
        );
    }

    #[test]
    fn the_bundles_document_lists_every_bundle() {
        let resolved =
            resolve(&ConfigFile::default(), "testhost", &CliOverrides::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&to_string(&bundles_view(&resolved, &ATOMIC))).unwrap();

        assert_eq!(v["bundles"].as_array().unwrap().len(), BUNDLES.len());
        assert_eq!(v["bundles"][0]["name"], "core");
        assert_eq!(v["bundles"][0]["enabled"], true);
    }

    #[test]
    fn the_config_document_reports_the_platform() {
        let resolved =
            resolve(&ConfigFile::default(), "testhost", &CliOverrides::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&to_string(&config_view(&resolved, &ATOMIC))).unwrap();

        assert_eq!(v["platform"]["atomic"], true);
        assert_eq!(v["upgrade"], false);
    }

    #[test]
    fn the_report_document_keys_are_stable() {
        let report = RunReport::default();

        let json = to_string(&report_view(&report));

        assert_eq!(
            keys(&json),
            vec!["caveats", "duration_secs", "steps", "warnings"]
        );
    }

    #[test]
    fn actions_report_whether_they_need_a_password() {
        // So a consumer can see what a run will ask for before starting it.
        let plan = ActionPlan {
            actions: vec![Action::Install {
                manager: ManagerId::Dnf,
                packages: vec!["xdotool".into()],
            }],
            dotfiles: vec![crate::managers::Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        let v: serde_json::Value =
            serde_json::from_str(&to_string(&plan_view(&plan, true))).unwrap();

        assert_eq!(v["actions"][0]["privileged"], true, "dnf needs sudo");
        assert_eq!(
            v["actions"][1]["privileged"], false,
            "this chezmoi step does not"
        );
    }
}
