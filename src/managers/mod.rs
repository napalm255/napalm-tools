//! Package managers `nt` can drive.

pub mod brew;
pub mod brew_cask;
pub mod bun;
pub mod dnf;
pub mod flatpak;
pub mod npm;

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fmt;

use crate::platform::Platform;

/// Identifies a package manager.
///
/// Ordering here carries no meaning; preference is expressed per-package by
/// the order of a package's providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ManagerId {
    /// Homebrew formulae.
    Brew,
    /// Homebrew casks. A separate namespace from formulae, not a variant of them.
    BrewCask,
    /// npm global installs.
    Npm,
    /// bun global installs.
    Bun,
    /// Flatpak.
    Flatpak,
    /// dnf. Never available on ostree-based systems.
    Dnf,
}

impl ManagerId {
    /// Every manager, in a stable order for reporting.
    pub const ALL: &'static [ManagerId] = &[
        ManagerId::Brew,
        ManagerId::BrewCask,
        ManagerId::Npm,
        ManagerId::Bun,
        ManagerId::Flatpak,
        ManagerId::Dnf,
    ];

    /// The manager's name as it appears in output and configuration.
    pub fn as_str(&self) -> &'static str {
        match self {
            ManagerId::Brew => "brew",
            ManagerId::BrewCask => "brew-cask",
            ManagerId::Npm => "npm",
            ManagerId::Bun => "bun",
            ManagerId::Flatpak => "flatpak",
            ManagerId::Dnf => "dnf",
        }
    }
}

impl fmt::Display for ManagerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A command line, kept as data so it can be rendered for `--dry-run` and
/// asserted on in tests without spawning anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    /// The program to run.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl Cmd {
    /// Build a command from a program and its arguments.
    pub fn new<I, S>(program: &str, args: I) -> Cmd
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Cmd {
            program: program.to_string(),
            args: args.into_iter().map(|a| a.as_ref().to_string()).collect(),
        }
    }

    /// Render as a shell-quoted command line, for display.
    pub fn to_shell(&self) -> String {
        let mut out = shell_quote(&self.program);
        for a in &self.args {
            out.push(' ');
            out.push_str(&shell_quote(a));
        }
        out
    }

    /// Convert into a runnable process command.
    pub fn to_command(&self) -> std::process::Command {
        let mut c = std::process::Command::new(&self.program);
        c.args(&self.args);
        c
    }

    /// Run the command, returning its stdout. Fails with the captured stderr
    /// tail so a subprocess failure is diagnosable from the error alone.
    pub fn output(&self) -> Result<String> {
        let out = self
            .to_command()
            .output()
            .with_context(|| format!("failed to run `{}`", self.to_shell()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = stderr.lines().rev().take(10).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            anyhow::bail!("`{}` failed: {}", self.to_shell(), tail.join("\n"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_shell())
    }
}

/// The manager implementation for an id.
pub fn get(id: ManagerId) -> Box<dyn Manager> {
    match id {
        ManagerId::Brew => Box::new(brew::Brew),
        ManagerId::BrewCask => Box::new(brew_cask::BrewCask),
        ManagerId::Npm => Box::new(npm::Npm),
        ManagerId::Bun => Box::new(bun::Bun),
        ManagerId::Flatpak => Box::new(flatpak::Flatpak),
        ManagerId::Dnf => Box::new(dnf::Dnf),
    }
}

/// Every manager implementation, in [`ManagerId::ALL`] order.
pub fn all() -> Vec<Box<dyn Manager>> {
    ManagerId::ALL.iter().copied().map(get).collect()
}

/// Parse newline-delimited command output into a set, ignoring blank lines and
/// surrounding whitespace. Shared by the managers whose listing commands emit
/// one name per line.
pub fn parse_lines(output: &str) -> HashSet<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Quote a single word for display in a shell command line.
fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@+,".contains(c));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// Whether `binary` is present on `PATH`.
pub fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(binary);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

/// A package manager `nt` can query and drive.
pub trait Manager {
    /// Which manager this is.
    fn id(&self) -> ManagerId;

    /// The binary that must be on `PATH` for this manager to work.
    fn binary(&self) -> &'static str;

    /// Whether this manager is usable on `platform`, ignoring `PATH`.
    ///
    /// Kept separate from [`Manager::available`] because the interesting cases
    /// are platform rules, not binary presence: `dnf` is on `PATH` under an
    /// ostree-based OS and will appear to work.
    fn platform_ok(&self, platform: &Platform) -> bool;

    /// Whether this manager can be used here.
    fn available(&self, platform: &Platform) -> bool {
        self.platform_ok(platform) && on_path(self.binary())
    }

    /// Every package this manager currently has installed, in one bulk query.
    fn installed(&self) -> Result<HashSet<String>>;

    /// Command to install the given packages.
    fn install_cmd(&self, packages: &[String]) -> Cmd;

    /// Command to upgrade the given packages.
    fn upgrade_cmd(&self, packages: &[String]) -> Cmd;

    /// Taps currently configured. Only meaningful for Homebrew.
    fn installed_taps(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    /// Command to add a tap. Only meaningful for Homebrew.
    fn tap_cmd(&self, _tap: &str) -> Option<Cmd> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_command_renders_without_quoting() {
        let c = Cmd::new("brew", ["install", "ripgrep"]);

        assert_eq!(c.to_shell(), "brew install ripgrep");
    }

    #[test]
    fn an_argument_with_spaces_is_quoted() {
        let c = Cmd::new("chezmoi", ["apply", "some path"]);

        assert_eq!(c.to_shell(), "chezmoi apply 'some path'");
    }

    #[test]
    fn a_single_quote_in_an_argument_is_escaped() {
        let c = Cmd::new("echo", ["it's"]);

        // Rendered output must be safe to paste into a shell.
        assert_eq!(c.to_shell(), r#"echo 'it'\''s'"#);
    }

    #[test]
    fn an_empty_argument_is_quoted_so_it_survives() {
        let c = Cmd::new("prog", [""]);

        assert_eq!(c.to_shell(), "prog ''");
    }

    #[test]
    fn a_command_with_no_arguments_is_just_the_program() {
        let c: Cmd = Cmd::new("brew", Vec::<String>::new());

        assert_eq!(c.to_shell(), "brew");
    }

    #[test]
    fn manager_names_round_trip() {
        for m in ManagerId::ALL {
            assert_eq!(m.to_string(), m.as_str());
        }
    }

    #[test]
    fn the_registry_returns_the_manager_that_was_asked_for() {
        for id in ManagerId::ALL {
            assert_eq!(get(*id).id(), *id);
        }
    }

    #[test]
    fn the_registry_covers_every_manager() {
        assert_eq!(all().len(), ManagerId::ALL.len());
    }

    #[test]
    fn every_manager_declares_a_binary() {
        for m in all() {
            assert!(!m.binary().is_empty(), "{} has no binary", m.id());
        }
    }
}
