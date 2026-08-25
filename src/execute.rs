//! Running a plan, and taking the snapshot a plan is built from.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};

use crate::managers::{self, Cmd, ManagerId};
use crate::plan::{ActionPlan, Snapshot};
use crate::platform::Platform;

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
pub fn run(plan: &ActionPlan, quiet: bool) -> Result<()> {
    let mut commands: Vec<Cmd> = plan.actions.iter().map(|a| a.to_cmd()).collect();
    // Dotfiles come last, so chezmoi itself can have been installed by the
    // package actions above on a fresh machine.
    commands.extend(plan.dotfiles.iter().cloned());
    run_commands(&commands, quiet)
}

/// Run a sequence of commands, stopping at the first failure.
///
/// Output is inherited rather than captured so package managers can show their
/// own progress; a failure is reported with the command that caused it.
pub fn run_commands(commands: &[Cmd], quiet: bool) -> Result<()> {
    for cmd in commands {
        if !quiet {
            println!("  + {}", cmd.to_shell());
        }
        let status = cmd
            .to_command()
            .status()
            .with_context(|| format!("failed to run `{}`", cmd.to_shell()))?;
        if !status.success() {
            anyhow::bail!("`{}` exited with {}", cmd.to_shell(), status);
        }
    }
    Ok(())
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
