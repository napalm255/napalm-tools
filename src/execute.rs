//! Running a plan, and taking the snapshot a plan is built from.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};

use crate::managers::{self, Cmd, ManagerId};
use crate::plan::{ActionPlan, Snapshot};
use crate::platform::Platform;
use crate::ui::Ui;
use crate::ui::scan::{Findings, Scanner};

/// What one step of a run did.
#[derive(Debug, Clone)]
pub struct StepOutcome {
    /// The command, as it would be typed.
    pub command: String,
    /// How long it took.
    pub duration: std::time::Duration,
    /// Whether it succeeded.
    pub success: bool,
}

/// What a whole run did, for the end-of-run summary.
#[derive(Debug, Clone, Default)]
pub struct RunReport {
    /// Each step, in order.
    pub steps: Vec<StepOutcome>,
    /// Anything worth surfacing from the managers' output.
    pub findings: Findings,
    /// Wall-clock time for the run.
    pub total: std::time::Duration,
}

/// Query every manager for what it has installed.
///
/// A manager that is unavailable here is skipped entirely, so no subprocess is
/// spawned for it. One that is available but fails to answer is a hard error:
/// planning against a half-known world would install things twice.
pub fn snapshot(platform: &Platform) -> Result<Snapshot> {
    let mut snap = Snapshot::default();

    for manager in managers::all() {
        let id = manager.id();
        if !manager.available(platform) {
            tracing::debug!(manager = %id, "not available on this host");
            continue;
        }
        snap.available.insert(id);

        let installed = manager
            .installed()
            .with_context(|| format!("failed to list packages installed by {id}"))?;
        tracing::debug!(manager = %id, count = installed.len(), "listed installed packages");
        snap.installed.insert(id, installed);

        if id == ManagerId::Brew {
            snap.taps = manager
                .installed_taps()
                .with_context(|| format!("failed to list {id} taps"))?;
        }
    }

    snap.binaries = binaries_on_path();
    Ok(snap)
}

/// Which of the catalog's declared executables resolve on `PATH`.
///
/// This is what lets a tool provided by the OS image, or installed by a
/// vendor script, count as satisfied instead of being installed a second time.
fn binaries_on_path() -> HashSet<String> {
    crate::bundles::BUNDLES
        .iter()
        .flat_map(|b| b.packages)
        .filter_map(|p| p.binary)
        .filter(|b| managers::on_path(b))
        .map(str::to_string)
        .collect()
}

/// Run every action in a plan, in order.
pub fn run(plan: &ActionPlan, ui: &Ui) -> Result<RunReport> {
    // Ask for the password before anything runs and before the spinner starts,
    // so a refusal costs nothing and the prompt is the only thing on screen.
    if crate::privilege::plan_needs_privileges(plan) && !crate::privilege::already_authorised() {
        ui.line("Some steps need elevated privileges.");
        crate::privilege::prime()?;
    }

    let mut commands: Vec<Cmd> = plan.actions.iter().map(|a| a.to_cmd()).collect();
    // Dotfiles come last, so chezmoi itself can have been installed by the
    // package actions above on a fresh machine.
    commands.extend(plan.dotfiles.iter().cloned());
    run_commands(&commands, ui)
}

/// Whether a failure looks like a command that wanted to ask a question.
///
/// Used only to attach a hint; getting it wrong costs a missing or spurious
/// line of advice, nothing more.
pub fn looks_like_a_prompt_failure(tail: &str) -> bool {
    const SIGNS: &[&str] = &[
        "a terminal is required",
        "terminal prompts disabled",
        "no tty present",
        "askpass",
        "a password is required",
        "could not read Username",
        "could not read Password",
        "Permission denied (publickey",
        "Host key verification failed",
    ];
    let lower = tail.to_ascii_lowercase();
    SIGNS
        .iter()
        .any(|s| lower.contains(&s.to_ascii_lowercase()))
}

/// Run a sequence of commands, stopping at the first failure.
///
/// Output is captured by default and scanned for the few lines worth showing;
/// under `-v` it streams through untouched instead. A failure carries the
/// command and the tail of its output, so capturing leaves a failure easier to
/// diagnose than inheriting stdio did, not harder.
pub fn run_commands(commands: &[Cmd], ui: &Ui) -> Result<RunReport> {
    let started = std::time::Instant::now();
    let mut report = RunReport::default();
    let total = commands.len();

    for (i, cmd) in commands.iter().enumerate() {
        let label = cmd.to_shell();
        let step = ui.step(i + 1, total, &label);

        let outcome = if ui.raw_subprocess_output() {
            cmd.run_streaming()?
        } else {
            let mut scanner = Scanner::new(label.clone());
            let outcome = cmd.run_captured(|line| {
                step.detail(line);
                scanner.line(line);
            })?;
            let found = scanner.finish();
            report.findings.caveats.extend(found.caveats);
            report.findings.warnings.extend(found.warnings);
            outcome
        };

        step.finish(outcome.success, outcome.duration);
        report.steps.push(StepOutcome {
            command: label.clone(),
            duration: outcome.duration,
            success: outcome.success,
        });

        if !outcome.success {
            report.total = started.elapsed();
            let tail = outcome.tail_text();
            if tail.is_empty() {
                anyhow::bail!("`{label}` {}", outcome.status);
            }
            if looks_like_a_prompt_failure(&tail) {
                anyhow::bail!(
                    "`{label}` {}\n{tail}\n\nhint: this command wanted to prompt; \
                     re-run with -v to give it the terminal",
                    outcome.status
                );
            }
            anyhow::bail!("`{label}` {}\n{tail}", outcome.status);
        }
    }

    report.total = started.elapsed();
    Ok(report)
}

/// Packages each manager reports as explicitly requested, for `nt status`.
///
/// Homebrew is the only manager that distinguishes explicit installs from
/// dependencies, so it is the only one where this differs from [`snapshot`].
pub fn explicit_packages(platform: &Platform) -> Result<BTreeMap<ManagerId, usize>> {
    let mut counts = BTreeMap::new();
    for manager in managers::all() {
        if !manager.available(platform) {
            continue;
        }
        let count = if manager.id() == ManagerId::Brew {
            managers::brew::Brew.leaves()?.len()
        } else {
            manager.installed()?.len()
        };
        counts.insert(manager.id(), count);
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Format, Ui};

    fn ui() -> Ui {
        Ui::capturing(Format::Plain).0
    }

    #[test]
    fn every_command_is_recorded_in_order() {
        let cmds = vec![
            Cmd::new("sh", ["-c", "echo one"]),
            Cmd::new("sh", ["-c", "echo two"]),
        ];

        let report = run_commands(&cmds, &ui()).unwrap();

        assert_eq!(report.steps.len(), 2);
        assert!(report.steps.iter().all(|s| s.success));
        assert!(report.steps[0].command.contains("echo one"));
    }

    #[test]
    fn a_failure_stops_the_run() {
        let cmds = vec![
            Cmd::new("sh", ["-c", "exit 1"]),
            Cmd::new("sh", ["-c", "echo should-not-run"]),
        ];

        let err = run_commands(&cmds, &ui()).unwrap_err();

        assert!(format!("{err:#}").contains("exit"), "got {err:#}");
    }

    #[test]
    fn a_failure_carries_the_output_tail() {
        // Capturing output must not make a failure harder to diagnose.
        let cmds = vec![Cmd::new(
            "sh",
            ["-c", "echo 'something went wrong' 1>&2; exit 2"],
        )];

        let err = run_commands(&cmds, &ui()).unwrap_err();

        assert!(
            format!("{err:#}").contains("something went wrong"),
            "the tail should be in the error, got {err:#}"
        );
    }

    #[test]
    fn caveats_in_output_are_collected() {
        let cmds = vec![Cmd::new(
            "sh",
            ["-c", "echo '==> Caveats'; echo 'run this in your shell'"],
        )];

        let report = run_commands(&cmds, &ui()).unwrap();

        assert_eq!(
            report.findings.caveats.len(),
            1,
            "got {:?}",
            report.findings
        );
        assert_eq!(
            report.findings.caveats[0].lines,
            vec!["run this in your shell"]
        );
    }

    #[test]
    fn warnings_in_output_are_collected() {
        let cmds = vec![Cmd::new("sh", ["-c", "echo 'Warning: already installed'"])];

        let report = run_commands(&cmds, &ui()).unwrap();

        assert_eq!(report.findings.warnings.len(), 1);
    }

    #[test]
    fn an_empty_run_reports_nothing() {
        let report = run_commands(&[], &ui()).unwrap();

        assert!(report.steps.is_empty());
        assert!(report.findings.is_empty());
    }

    #[test]
    fn a_total_duration_is_recorded() {
        let report = run_commands(&[Cmd::new("sh", ["-c", "exit 0"])], &ui()).unwrap();

        assert!(report.total >= report.steps[0].duration);
    }

    #[test]
    fn a_prompt_related_failure_is_recognised() {
        // The exact strings these tools emit, as measured.
        for tail in [
            "sudo: a terminal is required to read the password",
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            "sudo: no tty present and no askpass program specified",
            "Host key verification failed.",
        ] {
            assert!(looks_like_a_prompt_failure(tail), "missed: {tail}");
        }
    }

    #[test]
    fn an_ordinary_failure_is_not_mistaken_for_one() {
        for tail in [
            "Error: No available formula with the name \"nosuchpkg\"",
            "npm error code E404",
            "disk quota exceeded",
        ] {
            assert!(!looks_like_a_prompt_failure(tail), "false positive: {tail}");
        }
    }

    #[test]
    fn a_prompt_failure_carries_the_hint() {
        let cmds = vec![Cmd::new(
            "sh",
            [
                "-c",
                "echo 'sudo: a terminal is required to read the password' 1>&2; exit 1",
            ],
        )];

        let err = run_commands(&cmds, &ui()).unwrap_err();

        assert!(format!("{err:#}").contains("re-run with -v"), "got {err:#}");
    }

    #[test]
    fn an_ordinary_failure_carries_no_hint() {
        let cmds = vec![Cmd::new(
            "sh",
            ["-c", "echo 'no such formula' 1>&2; exit 1"],
        )];

        let err = run_commands(&cmds, &ui()).unwrap_err();

        assert!(
            !format!("{err:#}").contains("re-run with -v"),
            "got {err:#}"
        );
    }
}
