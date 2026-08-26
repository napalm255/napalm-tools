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
    plan.actions.iter().any(|a| a.to_cmd().privileged) || plan.dotfiles.iter().any(|c| c.privileged)
}

/// Whether any chezmoi run script invokes `sudo`.
///
/// A heuristic, and deliberately a shallow one: these are the user's own
/// scripts, so the only way to know is to look, and parsing shell to be sure
/// would be worse than the problem. Both failure modes are mild - a false
/// positive costs one unnecessary password prompt, and a false negative leaves
/// that step behaving as it did before, prompting on a terminal it still has.
pub fn scripts_use_sudo(source_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(source_dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Only `run_` scripts execute; anything else is just a dotfile.
        if !name.starts_with("run_") {
            return false;
        }
        std::fs::read_to_string(entry.path()).is_ok_and(|text| mentions_sudo(&text))
    })
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
}
