//! Deciding whether a run needs elevated privileges, and asking once if so.
//!
//! A password prompt appearing part-way through a run is the problem this
//! module exists to avoid: it arrives on `/dev/tty`, underneath a live
//! spinner, where it is invisible or garbled. Asking up front instead means a
//! refused password costs nothing, because nothing has run yet.

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::managers::Cmd;
use crate::plan::ActionPlan;

/// Whether any step in the plan may need elevated privileges.
pub fn plan_needs_privileges(plan: &ActionPlan) -> bool {
    plan.commands().iter().any(|c| c.privileged)
}

/// Whether any chezmoi run script invokes `sudo`.
///
/// Looks in the source directory itself and in `.chezmoiscripts/`, the
/// documented home for scripts that should not also be dotfiles.
///
/// A heuristic, and deliberately a shallow one: these are the user's own
/// scripts, so the only way to know is to look, and parsing shell to be sure
/// would be worse than the problem. Both failure modes are mild - a false
/// positive costs one unnecessary password prompt, and a false negative leaves
/// that step behaving as it did before, prompting on a terminal it still has.
pub fn scripts_use_sudo(source_dir: &Path) -> bool {
    run_scripts(source_dir)
        .into_iter()
        .any(|path| std::fs::read_to_string(path).is_ok_and(|text| mentions_sudo(&text)))
}

/// Every `run_*` script under `source_dir`, one level deep into
/// `.chezmoiscripts/`.
fn run_scripts(source_dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for dir in [source_dir.to_path_buf(), source_dir.join(".chezmoiscripts")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            // Only `run_` scripts execute; anything else is just a dotfile.
            if name.to_string_lossy().starts_with("run_") && entry.path().is_file() {
                found.push(entry.path());
            }
        }
    }
    found
}

/// Whether the script text invokes sudo, ignoring comment lines.
fn mentions_sudo(text: &str) -> bool {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .any(|line| {
            line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
                .any(|word| word == "sudo")
        })
}

/// Whether the effective user is root.
///
/// Read from `/proc` so no crate is needed for a single syscall's worth of
/// information. `NT_FAKE_UID=0` forces the answer to true so a test can
/// exercise the refusal; it can never make a real root look like an ordinary
/// user, because the guard exists to protect the machine from exactly the
/// caller who controls the environment.
pub fn is_root() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").ok();
    is_root_from(
        status.as_deref().and_then(uid_from_proc_status),
        std::env::var("NT_FAKE_UID").ok().as_deref(),
    )
}

/// The pure decision: root if the real uid is 0, or if the override forces it.
fn is_root_from(real_uid: Option<&str>, forced: Option<&str>) -> bool {
    real_uid == Some("0") || forced == Some("0")
}

/// The effective uid from the text of `/proc/self/status`: the third field
/// of the `Uid:` line (real, effective, saved, filesystem).
fn uid_from_proc_status(text: &str) -> Option<&str> {
    text.lines()
        .find(|l| l.starts_with("Uid:"))?
        .split_whitespace()
        .nth(2)
}

/// Whether sudo already holds a valid cached credential.
pub fn already_authorised() -> bool {
    Cmd::new("sudo", ["-n", "true"])
        .privileged()
        .run_captured(|_| {})
        .map(|o| o.success)
        .unwrap_or(false)
}

/// Ask for the password once, with the terminal inherited.
///
/// Called before any step runs and before the spinner starts, so the prompt is
/// the only thing on screen.
pub fn prime() -> Result<()> {
    let outcome = Cmd::new("sudo", ["-v"])
        .privileged()
        .run_streaming()
        .context("failed to run `sudo -v`")?;
    if !outcome.success {
        bail!(
            "this run needs elevated privileges, and `sudo -v` {}",
            outcome.status
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::ManagerId;
    use crate::plan::Action;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn a_run_script_calling_sudo_is_detected() {
        // Shaped like the real one in the dotfiles this was found in.
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "run_onchange_after_install-cert.sh.tmpl",
            "#!/bin/bash\necho 'Installing custom certificate...'\nsudo cp x /etc/pki/y\n",
        );

        assert!(scripts_use_sudo(dir.path()));
    }

    #[test]
    fn a_run_script_without_sudo_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "run_after_hello.sh",
            "#!/bin/bash\necho hello\n",
        );

        assert!(!scripts_use_sudo(dir.path()));
    }

    #[test]
    fn sudo_mentioned_only_in_a_comment_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "run_after_note.sh",
            "#!/bin/bash\n# this deliberately avoids sudo\necho hi\n",
        );

        assert!(!scripts_use_sudo(dir.path()));
    }

    #[test]
    fn a_word_merely_containing_sudo_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "run_after_x.sh",
            "echo pseudonym; echo sudoku\n",
        );

        assert!(!scripts_use_sudo(dir.path()));
    }

    #[test]
    fn a_script_in_the_chezmoiscripts_directory_is_scanned() {
        // chezmoi's documented home for scripts; the common layout.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".chezmoiscripts")).unwrap();
        write(
            &dir.path().join(".chezmoiscripts"),
            "run_once_after_setup.sh",
            "#!/bin/bash\nsudo systemctl enable something\n",
        );

        assert!(scripts_use_sudo(dir.path()));
    }

    #[test]
    fn a_non_run_file_is_ignored() {
        // Only run_ scripts execute; a dotfile mentioning sudo is just text.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "dot_bashrc", "alias please='sudo'\n");

        assert!(!scripts_use_sudo(dir.path()));
    }

    #[test]
    fn an_empty_source_directory_needs_nothing() {
        let dir = tempfile::tempdir().unwrap();

        assert!(!scripts_use_sudo(dir.path()));
    }

    #[test]
    fn a_missing_source_directory_needs_nothing() {
        assert!(!scripts_use_sudo(Path::new("/nonexistent/chezmoi")));
    }

    #[test]
    fn a_plan_with_a_privileged_action_needs_privileges() {
        let plan = ActionPlan {
            actions: vec![Action::Install {
                manager: ManagerId::Dnf,
                packages: vec!["xdotool".into()],
            }],
            ..Default::default()
        };

        assert!(plan_needs_privileges(&plan));
    }

    #[test]
    fn a_privileged_bootstrap_step_needs_privileges() {
        let plan = ActionPlan {
            bootstrap: vec![Cmd::new("sudo", ["dnf", "install", "-y", "git"]).privileged()],
            ..Default::default()
        };

        assert!(plan_needs_privileges(&plan));
    }

    #[test]
    fn a_plan_of_ordinary_actions_needs_nothing() {
        let plan = ActionPlan {
            actions: vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["ripgrep".into()],
            }],
            ..Default::default()
        };

        assert!(!plan_needs_privileges(&plan));
    }

    #[test]
    fn a_privileged_dotfiles_step_needs_privileges() {
        let plan = ActionPlan {
            dotfiles: vec![Cmd::new("chezmoi", ["apply"]).privileged()],
            ..Default::default()
        };

        assert!(plan_needs_privileges(&plan));
    }

    #[test]
    fn an_ordinary_dotfiles_step_does_not() {
        let plan = ActionPlan {
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        assert!(!plan_needs_privileges(&plan));
    }

    #[test]
    fn an_empty_plan_needs_nothing() {
        assert!(!plan_needs_privileges(&ActionPlan::default()));
    }

    const STATUS: &str =
        "Name:\tnt\nUmask:\t0022\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";

    #[test]
    fn the_effective_uid_is_the_second_number_on_the_uid_line() {
        // A setuid binary has real 1000 but effective 0; the effective one
        // is what decides who owns the files nt would create.
        let text = STATUS.replace("Uid:\t1000\t1000", "Uid:\t1000\t0");

        assert_eq!(uid_from_proc_status(&text), Some("0"));
        assert_eq!(uid_from_proc_status(STATUS), Some("1000"));
    }

    #[test]
    fn a_status_without_a_uid_line_yields_nothing() {
        assert_eq!(uid_from_proc_status("Name:\tnt\n"), None);
        assert_eq!(uid_from_proc_status(""), None);
    }

    #[test]
    fn a_forced_root_uid_makes_an_ordinary_user_look_like_root() {
        assert!(is_root_from(Some("1000"), Some("0")));
    }

    #[test]
    fn a_fake_non_root_uid_cannot_hide_a_real_root() {
        // The property the guard depends on: the environment can only
        // tighten the check, never loosen it.
        assert!(is_root_from(Some("0"), Some("1000")));
        assert!(is_root_from(Some("0"), None));
        assert!(is_root_from(Some("0"), Some("")));
    }

    #[test]
    fn an_ordinary_user_without_an_override_is_not_root() {
        assert!(!is_root_from(Some("1000"), None));
        assert!(!is_root_from(None, None));
        assert!(!is_root_from(Some("1000"), Some("1000")));
    }
}
