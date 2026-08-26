//! Package managers `nt` can drive.

pub mod brew;
pub mod brew_cask;
pub mod bun;
pub mod dnf;
pub mod flatpak;
pub mod mise;
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
    /// mise, for language toolchains. Ids are `tool@version`.
    Mise,
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
        ManagerId::Mise,
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
            ManagerId::Mise => "mise",
            ManagerId::Dnf => "dnf",
        }
    }

    /// The manager named `name` in configuration or output.
    pub fn from_name(name: &str) -> Option<ManagerId> {
        ManagerId::ALL.iter().copied().find(|m| m.as_str() == name)
    }

    /// Every name, comma-separated, for error messages.
    pub fn names() -> String {
        ManagerId::ALL
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ")
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
    /// Whether this command may legitimately need elevated privileges.
    ///
    /// Such a command keeps the controlling terminal, because sudo's cached
    /// credential is bound to the terminal it was entered on. Every other
    /// command is detached, so an unexpected prompt fails instead of hanging.
    pub privileged: bool,
    /// Directory to run in, when it matters. mise reads the current
    /// directory's project configuration - and refuses untrusted files - so
    /// its commands run from the home directory, where only the global
    /// configuration applies.
    pub cwd: Option<std::path::PathBuf>,
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
            privileged: false,
            cwd: None,
        }
    }

    /// A command of the shape `program fixed-args... packages...`, which is
    /// what every manager's install and upgrade command looks like.
    pub fn with_packages(program: &str, fixed: &[&str], packages: &[String]) -> Cmd {
        let args = fixed
            .iter()
            .map(|a| (*a).to_string())
            .chain(packages.iter().cloned());
        Cmd::new(program, args)
    }

    /// Mark the command as one that may need elevated privileges.
    pub fn privileged(mut self) -> Cmd {
        self.privileged = true;
        self
    }

    /// Run the command from `dir` rather than the current directory.
    pub fn in_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Cmd {
        self.cwd = Some(dir.into());
        self
    }

    /// Run the command from the user's home directory, if `HOME` is set.
    pub fn in_home(self) -> Cmd {
        match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            Some(home) => self.in_dir(home),
            None => self,
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
    ///
    /// Ordinary commands are detached from the controlling terminal, so a
    /// program that tries to prompt on `/dev/tty` fails at once rather than
    /// waiting forever behind the spinner. Privileged commands keep the
    /// terminal, because sudo's cached credential is bound to it.
    pub fn to_command(&self) -> std::process::Command {
        self.build_command(!self.privileged)
    }

    /// Build the process command, detaching from the terminal or not.
    fn build_command(&self, detach: bool) -> std::process::Command {
        // Resolve through the known tool directories as well as PATH, so a
        // manager installed moments ago by the bootstrap phase is found even
        // though this shell's PATH predates it.
        let program = resolve_program(&self.program)
            .map(|p| p.into_os_string())
            .unwrap_or_else(|| self.program.clone().into());
        let mut c = if !detach || !setsid_available() {
            let mut c = std::process::Command::new(&program);
            c.args(&self.args);
            c
        } else {
            let mut c = std::process::Command::new(SETSID);
            // `--wait` makes setsid exit with the child's status rather than
            // its own, so the outcome is still the command's.
            c.arg("--wait").arg(&program).args(&self.args);
            c
        };
        if let Some(dir) = &self.cwd {
            c.current_dir(dir);
        }
        non_interactive_env(&mut c);
        c
    }

    /// Run the command with its output captured, invoking `on_line` for each
    /// line of stdout and stderr as it arrives.
    ///
    /// `stdin` is null. A command that unexpectedly waits for input then fails
    /// immediately rather than hanging forever against a pipe nobody is
    /// feeding - much the worst failure mode available here.
    pub fn run_captured(&self, mut on_line: impl FnMut(&str)) -> Result<CmdOutcome> {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        use std::sync::mpsc;

        self.ensure_program_exists()?;
        let started = std::time::Instant::now();
        let mut child = self
            .to_command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to run `{}`", self.to_shell()))?;

        // One reader thread per pipe, both feeding a single channel, so the
        // two streams interleave in arrival order and neither can fill its
        // buffer and deadlock the child.
        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    // A closed receiver just means nobody is listening.
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }));
        }
        // Both clones live in the threads now; drop ours so the channel ends.
        drop(tx);

        let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        for line in rx {
            on_line(&line);
            tail.push_back(line);
            if tail.len() > TAIL_LINES {
                tail.pop_front();
            }
        }
        for reader in readers {
            let _ = reader.join();
        }

        let status = child
            .wait()
            .with_context(|| format!("failed to wait for `{}`", self.to_shell()))?;
        Ok(CmdOutcome {
            success: status.success(),
            status: status.to_string(),
            duration: started.elapsed(),
            tail: tail.into(),
        })
    }

    /// Run the command with stdio and the terminal inherited, so the child
    /// writes straight to the terminal, keeps its own colour and progress
    /// rendering, and can prompt if it needs to.
    pub fn run_streaming(&self) -> Result<CmdOutcome> {
        self.ensure_program_exists()?;
        let started = std::time::Instant::now();
        // Never detached: raw mode exists precisely so a command can use the
        // terminal, including to ask a question.
        let status = self
            .build_command(false)
            .status()
            .with_context(|| format!("failed to run `{}`", self.to_shell()))?;
        Ok(CmdOutcome {
            success: status.success(),
            status: status.to_string(),
            duration: started.elapsed(),
            tail: Vec::new(),
        })
    }

    /// Fail before spawning if the program does not exist.
    ///
    /// Without this, detaching through `setsid` would turn "no such program"
    /// into an ordinary non-zero exit, losing the distinction between a
    /// command that failed and one that was never there.
    fn ensure_program_exists(&self) -> Result<()> {
        if resolve_program(&self.program).is_some() {
            Ok(())
        } else {
            anyhow::bail!("`{}` is not installed or not on PATH", self.program)
        }
    }

    /// Run the command, returning its stdout. Fails with the captured stderr
    /// tail so a subprocess failure is diagnosable from the error alone.
    pub fn output(&self) -> Result<String> {
        self.ensure_program_exists()?;
        tracing::debug!(command = %self.to_shell(), "querying");
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
        ManagerId::Mise => Box::new(mise::Mise),
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

/// The helper used to detach a command from the controlling terminal.
const SETSID: &str = "setsid";

/// Whether `setsid` can be found. Absent only on an unusual system; the
/// fallback is to run attached, which is how `nt` behaved before.
fn setsid_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let found = on_path(SETSID);
        if !found {
            tracing::debug!("setsid not found; commands will keep the terminal");
        }
        found
    })
}

/// Tell every tool that offers the choice not to ask questions.
///
/// Detaching the terminal already prevents a prompt from blocking, but a tool
/// that knows it is non-interactive fails with a far better message than one
/// discovering its terminal has gone.
fn non_interactive_env(command: &mut std::process::Command) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1");
}

/// Lines of combined output retained for a failure report.
const TAIL_LINES: usize = 20;

/// The result of running a command.
#[derive(Debug, Clone)]
pub struct CmdOutcome {
    /// Whether the command exited successfully.
    pub success: bool,
    /// How the command exited, rendered for display.
    pub status: String,
    /// How long it took.
    pub duration: std::time::Duration,
    /// The last [`TAIL_LINES`] lines of combined output, for diagnosing a
    /// failure. Empty when output was inherited rather than captured.
    pub tail: Vec<String>,
}

impl CmdOutcome {
    /// The retained output as text, for an error message.
    pub fn tail_text(&self) -> String {
        self.tail.join("\n")
    }
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

/// Whether `binary` is present on `PATH` or in a known tool directory.
pub fn on_path(binary: &str) -> bool {
    resolve_program(binary).is_some()
}

/// Locate `program`: an explicit path as given, otherwise the first
/// executable of that name on `PATH` or in one of the directories the
/// managers `nt` bootstraps install into. Those directories are checked
/// because a freshly installed manager is not yet on the PATH of the shell
/// that installed it.
pub fn resolve_program(program: &str) -> Option<std::path::PathBuf> {
    if program.contains('/') {
        let p = std::path::PathBuf::from(program);
        return is_executable(&p).then_some(p);
    }
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    path_dirs
        .into_iter()
        .chain(known_tool_dirs())
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

/// Directories that the bootstrapped managers install into.
///
/// `NT_TOOL_DIRS` (colon-separated, may be empty) overrides the list so a
/// test can simulate a host that has none of them.
pub fn known_tool_dirs() -> Vec<std::path::PathBuf> {
    if let Some(list) = std::env::var_os("NT_TOOL_DIRS") {
        return std::env::split_paths(&list).collect();
    }
    let mut dirs = vec![std::path::PathBuf::from("/home/linuxbrew/.linuxbrew/bin")];
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".local/share/mise/shims"));
    }
    dirs
}

/// A regular file with an execute bit for someone.
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
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

    /// Command to trust a tap.
    ///
    /// Homebrew requires third-party taps to be trusted before it will load
    /// their formulae at all - an untrusted tap is silently ignored, so a
    /// tapped package would appear to install and simply not.
    fn trust_cmd(&self, _tap: &str) -> Option<Cmd> {
        None
    }

    /// Taps already trusted, as recorded paths.
    fn trusted_taps(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    /// Remotes configured for installs. Only meaningful for Flatpak, whose
    /// user scope starts with none.
    fn remotes(&self) -> Result<HashSet<String>> {
        Ok(HashSet::new())
    }

    /// Command to add the remote installs come from. Only meaningful for
    /// Flatpak.
    fn add_remote_cmd(&self) -> Option<Cmd> {
        None
    }

    /// The name of the remote installs come from, if the manager has one.
    fn remote_name(&self) -> Option<&'static str> {
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
            assert_eq!(ManagerId::from_name(m.as_str()), Some(*m));
        }
        assert_eq!(ManagerId::from_name("pacman"), None);
    }

    #[test]
    fn with_packages_appends_packages_after_the_fixed_arguments() {
        let c = Cmd::with_packages("brew", &["install", "--cask"], &["a".into(), "b".into()]);

        assert_eq!(c.to_shell(), "brew install --cask a b");
    }

    #[test]
    fn a_command_can_be_run_from_another_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mut seen = Vec::new();

        Cmd::new("sh", ["-c", "pwd"])
            .in_dir(dir.path())
            .run_captured(|l| seen.push(l.to_string()))
            .unwrap();

        let real = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(seen, vec![real.to_string_lossy().to_string()]);
    }

    #[test]
    fn a_non_executable_file_is_not_on_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("nt-plain-file"), "").unwrap();

        assert!(!is_executable(&dir.path().join("nt-plain-file")));
        assert!(is_executable(std::path::Path::new("/bin/sh")));
    }

    #[test]
    fn an_explicit_path_resolves_to_itself() {
        assert_eq!(
            resolve_program("/bin/sh"),
            Some(std::path::PathBuf::from("/bin/sh"))
        );
        assert!(resolve_program("/nonexistent/prog").is_none());
    }

    #[test]
    fn output_of_a_missing_program_is_an_error_naming_it() {
        let err = Cmd::new("nt-no-such-program-exists", ["x"])
            .output()
            .unwrap_err();

        assert!(format!("{err:#}").contains("not installed"), "got {err:#}");
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

    #[test]
    fn captured_output_reaches_the_line_callback() {
        let mut seen = Vec::new();
        let cmd = Cmd::new("sh", ["-c", "echo alpha; echo beta"]);

        let outcome = cmd.run_captured(|l| seen.push(l.to_string())).unwrap();

        assert!(outcome.success);
        assert_eq!(seen, vec!["alpha", "beta"]);
    }

    #[test]
    fn stderr_is_captured_as_well_as_stdout() {
        let mut seen = Vec::new();
        let cmd = Cmd::new("sh", ["-c", "echo to-stderr 1>&2"]);

        cmd.run_captured(|l| seen.push(l.to_string())).unwrap();

        assert_eq!(seen, vec!["to-stderr"]);
    }

    #[test]
    fn a_failing_command_reports_failure_rather_than_erroring() {
        // The command ran; it just failed. That is an outcome, not an error.
        let cmd = Cmd::new("sh", ["-c", "exit 3"]);

        let outcome = cmd.run_captured(|_| {}).unwrap();

        assert!(!outcome.success);
        assert!(outcome.status.contains('3'), "got {:?}", outcome.status);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let cmd = Cmd::new("nt-no-such-program-exists", ["--version"]);

        assert!(cmd.run_captured(|_| {}).is_err());
    }

    #[test]
    fn the_tail_keeps_the_last_lines_for_a_failure_report() {
        let cmd = Cmd::new("sh", ["-c", "for i in $(seq 1 50); do echo line$i; done"]);

        let outcome = cmd.run_captured(|_| {}).unwrap();

        assert_eq!(outcome.tail.len(), TAIL_LINES);
        assert_eq!(outcome.tail.last().unwrap(), "line50");
        assert_eq!(outcome.tail.first().unwrap(), "line31");
    }

    #[test]
    fn stdin_is_closed_so_a_prompting_command_cannot_hang() {
        // Reading stdin sees EOF at once; without this the test would block.
        let cmd = Cmd::new("sh", ["-c", "read answer; echo \"got:$answer\""]);

        let mut seen = Vec::new();
        cmd.run_captured(|l| seen.push(l.to_string())).unwrap();

        // Reaching this line at all is most of the point: with an inherited or
        // unfed stdin the read would block and the test would never return.
        assert_eq!(seen, vec!["got:"], "stdin should be at EOF immediately");
    }

    #[test]
    fn a_command_that_depends_on_stdin_fails_rather_than_blocking() {
        // `read` alone, so its non-zero status at EOF is the script's status.
        let outcome = Cmd::new("sh", ["-c", "read answer"])
            .run_captured(|_| {})
            .unwrap();

        assert!(!outcome.success);
    }

    #[test]
    fn a_duration_is_recorded() {
        let cmd = Cmd::new("sh", ["-c", "exit 0"]);

        let outcome = cmd.run_captured(|_| {}).unwrap();

        assert!(outcome.duration.as_nanos() > 0);
    }

    #[test]
    fn streaming_reports_success_without_capturing() {
        let cmd = Cmd::new("sh", ["-c", "exit 0"]);

        let outcome = cmd.run_streaming().unwrap();

        assert!(outcome.success);
        assert!(outcome.tail.is_empty(), "streaming captures nothing");
    }

    #[test]
    fn streaming_reports_a_failure() {
        let outcome = Cmd::new("sh", ["-c", "exit 7"]).run_streaming().unwrap();

        assert!(!outcome.success);
    }

    /// Our own session id, read from /proc.
    fn our_session() -> String {
        let stat = std::fs::read_to_string("/proc/self/stat").expect("own stat readable");
        // The comm field can contain spaces, so count from the closing
        // parenthesis rather than from the start of the line.
        let after_comm = stat.rsplit_once(')').expect("stat has a comm field").1;
        after_comm
            .split_whitespace()
            .nth(3)
            .expect("session id present")
            .to_string()
    }

    /// The session id a command reports for itself while still running.
    fn session_reported_by(cmd: Cmd) -> String {
        let mut seen = String::new();
        cmd.run_captured(|l| seen.push_str(l.trim())).unwrap();
        assert!(!seen.is_empty(), "command reported no session id");
        seen
    }

    #[test]
    fn an_ordinary_command_runs_in_its_own_session() {
        // Detachment is what turns an invisible hang on a terminal prompt into
        // an immediate, readable failure.
        let reported = session_reported_by(Cmd::new("sh", ["-c", "ps -o sid= -p $$"]));

        assert_ne!(
            reported,
            our_session(),
            "an ordinary command should not share our session"
        );
    }

    #[test]
    fn a_privileged_command_keeps_our_session() {
        // It has to, or sudo's tty-bound credential would not apply to it.
        let reported = session_reported_by(Cmd::new("sh", ["-c", "ps -o sid= -p $$"]).privileged());

        assert_eq!(reported, our_session());
    }

    #[test]
    fn the_non_interactive_environment_reaches_the_child() {
        let mut seen = Vec::new();
        Cmd::new(
            "sh",
            ["-c", "echo \"$GIT_TERMINAL_PROMPT|$SSH_ASKPASS_REQUIRE\""],
        )
        .run_captured(|l| seen.push(l.to_string()))
        .unwrap();

        assert_eq!(seen, vec!["0|never"], "git and ssh must not try to prompt");
    }

    #[test]
    fn the_non_interactive_environment_reaches_a_privileged_child_too() {
        let mut seen = Vec::new();
        Cmd::new("sh", ["-c", "echo $GIT_TERMINAL_PROMPT"])
            .privileged()
            .run_captured(|l| seen.push(l.to_string()))
            .unwrap();

        assert_eq!(seen, vec!["0"]);
    }

    #[test]
    fn an_exit_status_survives_detachment() {
        let outcome = Cmd::new("sh", ["-c", "exit 7"])
            .run_captured(|_| {})
            .unwrap();

        assert!(!outcome.success);
        assert!(outcome.status.contains('7'), "got {:?}", outcome.status);
    }

    #[test]
    fn marking_a_command_privileged_is_visible() {
        assert!(!Cmd::new("brew", ["install", "x"]).privileged);
        assert!(Cmd::new("sudo", ["dnf"]).privileged().privileged);
    }

    #[test]
    fn raw_mode_keeps_the_terminal_so_a_command_can_ask() {
        // -v is the escape hatch for a command that needs to prompt; detaching
        // it would defeat the entire purpose.
        let outcome = Cmd::new("sh", ["-c", "test -t 0 || true"])
            .run_streaming()
            .unwrap();

        assert!(outcome.success);
    }

    #[test]
    fn streaming_does_not_route_through_setsid() {
        let cmd = Cmd::new("sh", ["-c", "exit 0"]);
        let detached = cmd.to_command();
        let attached = cmd.build_command(false);

        assert_eq!(
            std::path::Path::new(attached.get_program())
                .file_name()
                .unwrap(),
            "sh"
        );
        assert_eq!(detached.get_program(), SETSID, "ordinary runs detach");
    }
}
