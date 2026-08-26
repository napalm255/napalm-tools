//! Flatpak.
//!
//! Installs go to the user scope so `nt` never mutates system state, but the
//! installed-check must consult **both** scopes: on a typical desktop the
//! existing applications were installed system-wide, and treating those as
//! missing would mean reinstalling every one of them as a user copy.
//!
//! The user scope starts with **no remotes** - on this development machine
//! and on any fresh Fedora - so an install there fails until Flathub has been
//! added to it. The plan adds it first.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId, parse_lines};
use crate::platform::Platform;

/// The Flatpak manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flatpak;

/// The remote applications are installed from.
pub const REMOTE: &str = "flathub";
/// Where that remote is defined.
pub const REMOTE_URL: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

/// Combine the user-scope and system-scope listings.
pub fn union_scopes(user: &str, system: &str) -> HashSet<String> {
    let mut set = parse_lines(user);
    set.extend(parse_lines(system));
    set
}

impl Manager for Flatpak {
    fn id(&self) -> ManagerId {
        ManagerId::Flatpak
    }

    fn binary(&self) -> &'static str {
        "flatpak"
    }

    fn platform_ok(&self, platform: &Platform) -> bool {
        // Applications need somewhere to draw; runtimes alone are no use.
        platform.graphical
    }

    fn installed(&self) -> Result<HashSet<String>> {
        // `--app` so runtimes and extensions are not mistaken for
        // applications; there are more of them than apps on a typical desktop.
        let user = Cmd::new(
            "flatpak",
            ["list", "--user", "--app", "--columns=application"],
        )
        .output()?;
        let system = Cmd::new(
            "flatpak",
            ["list", "--system", "--app", "--columns=application"],
        )
        .output()?;
        Ok(union_scopes(&user, &system))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages(
            "flatpak",
            &["install", "--user", "--noninteractive", REMOTE],
            packages,
        )
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages(
            "flatpak",
            &["update", "--user", "--noninteractive"],
            packages,
        )
    }

    fn remotes(&self) -> Result<HashSet<String>> {
        Ok(parse_lines(
            &Cmd::new("flatpak", ["remotes", "--user", "--columns=name"]).output()?,
        ))
    }

    fn add_remote_cmd(&self) -> Option<Cmd> {
        Some(Cmd::new(
            "flatpak",
            [
                "remote-add",
                "--user",
                "--if-not-exists",
                REMOTE,
                REMOTE_URL,
            ],
        ))
    }

    fn remote_name(&self) -> Option<&'static str> {
        Some(REMOTE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::test_platforms::*;

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
    fn installs_go_to_the_user_scope_from_flathub() {
        let cmd = Flatpak.install_cmd(&["com.spotify.Client".into()]);

        assert_eq!(
            cmd.to_shell(),
            "flatpak install --user --noninteractive flathub com.spotify.Client",
            "nt must never install into the system scope"
        );
    }

    #[test]
    fn the_remote_is_added_to_the_user_scope_idempotently() {
        let cmd = Flatpak.add_remote_cmd().unwrap();

        assert!(cmd.args.contains(&"--user".to_string()));
        assert!(cmd.args.contains(&"--if-not-exists".to_string()));
        assert!(cmd.to_shell().contains(REMOTE_URL));
        assert!(!cmd.privileged, "the user scope needs no password");
    }

    #[test]
    fn flatpak_needs_a_desktop() {
        assert!(Flatpak.platform_ok(&ATOMIC));
        assert!(Flatpak.platform_ok(&PLAIN));
        assert!(!Flatpak.platform_ok(&SERVER));
        assert!(!Flatpak.platform_ok(&CONTAINER));
        assert!(!Flatpak.platform_ok(&UNDER_WSL));
    }
}
