//! Flatpak.
//!
//! Installs go to the user scope so `nt` never mutates system state, but the
//! installed-check must consult **both** scopes: on a typical desktop the
//! existing applications were installed system-wide, and treating those as
//! missing would mean reinstalling every one of them as a user copy.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The Flatpak manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flatpak;

/// Parse `flatpak list --columns=application` output into application IDs.
pub fn parse_list(output: &str) -> HashSet<String> {
    super::parse_lines(output)
}

/// Combine the user-scope and system-scope listings.
pub fn union_scopes(user: &str, system: &str) -> HashSet<String> {
    let mut set = parse_list(user);
    set.extend(parse_list(system));
    set
}

impl Manager for Flatpak {
    fn id(&self) -> ManagerId {
        ManagerId::Flatpak
    }

    fn binary(&self) -> &'static str {
        "flatpak"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        let user = Cmd::new("flatpak", ["list", "--user", "--columns=application"]).output()?;
        let system = Cmd::new("flatpak", ["list", "--system", "--columns=application"]).output()?;
        Ok(union_scopes(&user, &system))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec![
            "install".to_string(),
            "--user".to_string(),
            "--noninteractive".to_string(),
        ];
        args.extend(packages.iter().cloned());
        Cmd::new("flatpak", args)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        let mut args = vec![
            "update".to_string(),
            "--user".to_string(),
            "--noninteractive".to_string(),
        ];
        args.extend(packages.iter().cloned());
        Cmd::new("flatpak", args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_application_ids() {
        let set = parse_list("com.spotify.Client\norg.remmina.Remmina\n");

        assert!(set.contains("com.spotify.Client"));
        assert!(set.contains("org.remmina.Remmina"));
    }

    #[test]
    fn a_system_scope_application_counts_as_installed() {
        // The case that matters: nothing in the user scope, everything in the
        // system scope. Treating these as missing would reinstall them all.
        let set = union_scopes("", "com.spotify.Client\ncom.google.Chrome\n");

        assert!(
            set.contains("com.spotify.Client"),
            "a system-scope flatpak must read as installed"
        );
    }

    #[test]
    fn both_scopes_are_combined_without_duplicates() {
        let set = union_scopes(
            "org.remmina.Remmina\n",
            "com.spotify.Client\norg.remmina.Remmina\n",
        );

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn installs_go_to_the_user_scope_only() {
        let cmd = Flatpak.install_cmd(&["com.spotify.Client".into()]);

        assert!(
            cmd.args.contains(&"--user".to_string()),
            "nt must never install into the system scope: {}",
            cmd.to_shell()
        );
        assert!(!cmd.args.contains(&"--system".to_string()));
    }

    #[test]
    fn installs_are_noninteractive() {
        let cmd = Flatpak.install_cmd(&["com.spotify.Client".into()]);

        assert!(cmd.args.contains(&"--noninteractive".to_string()));
    }
}
