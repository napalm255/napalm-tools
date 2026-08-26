//! Hermetic tests of a real `nt apply`, no `--dry-run`.
//!
//! Every package manager is a fake (`tests/fixtures/fake-bin/fake`) that
//! records what it was asked and answers listing queries from the
//! environment, so the whole converge path - bootstrap, snapshot, plan, run,
//! summary - executes without touching the network or this machine.
//!
//! `brew` and `mise` are not on the fake `PATH`; each test that wants them
//! links them into a per-test tool directory that nt searches through
//! `NT_TOOL_DIRS`, so the bootstrap path can also be exercised with them
//! genuinely absent. Bundles are all disabled (`--only core --skip core`) and
//! packages come from `[extra]`, so the plan never depends on which catalog
//! binaries the host happens to have.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// A throwaway machine: home, tool directory, call log.
struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    tools: PathBuf,
    log: PathBuf,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sandbox() -> Sandbox {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&tools).unwrap();
    Sandbox {
        log: dir.path().join("calls.log"),
        _dir: dir,
        home,
        tools,
    }
}

impl Sandbox {
    /// Put a fake `name` into the tool directory, as if a bootstrap had.
    fn provide(&self, name: &str) -> &Self {
        std::os::unix::fs::symlink(fixtures().join("fake-bin/fake"), self.tools.join(name))
            .unwrap();
        self
    }

    /// Homebrew and mise already present: no bootstrap will be planned.
    fn with_managers(&self) -> &Self {
        self.provide("brew").provide("mise")
    }

    /// Write the config `nt` will read: dotfiles off unless `body` says otherwise.
    fn config(&self, body: &str) -> PathBuf {
        let path = self.home.join("nt.toml");
        let text = if body.contains("[dotfiles]") {
            body.to_string()
        } else {
            format!("[dotfiles]\nenabled = false\n{body}")
        };
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Every fake invocation so far, in order.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

/// Where a test pretends to run.
#[derive(Clone, Copy)]
enum Host {
    /// Bluefin: atomic and graphical, so dnf is out and flatpak is in.
    Atomic,
    /// The Fedora container image: mutable and headless, so dnf is usable.
    Container,
}

/// `nt apply <config>` with every machine input pinned and the fakes on PATH.
fn apply(sb: &Sandbox, host: Host, config: &str) -> Command {
    let config = sb.config(config);
    let exists = fixtures().join("os-release-bluefin");
    let missing = PathBuf::from("/nonexistent/marker");
    let (os_release, atomic, container, graphical) = match host {
        Host::Atomic => ("os-release-bluefin", true, false, true),
        Host::Container => ("os-release-fedora", false, true, false),
    };
    let mut cmd = Command::cargo_bin("nt").unwrap();
    cmd.env_clear();
    // Under cargo-llvm-cov the spawned binary must keep writing its profile
    // where the run collects it, or its coverage is lost to a stray file.
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile);
    }
    cmd.env(
        "PATH",
        format!("{}:/usr/bin:/bin", fixtures().join("fake-bin").display()),
    )
    .env("HOME", &sb.home)
    .env("NT_TOOL_DIRS", &sb.tools)
    .env("FAKE_LOG", &sb.log)
    .env("FAKE_TOOL_DIR", &sb.tools)
    .env("NT_CONFIG", config)
    .env("NT_HOSTNAME", "testhost.example.com")
    .env("NT_OS_RELEASE", fixtures().join(os_release))
    .env("NT_OSTREE_MARKER", if atomic { &exists } else { &missing })
    .env(
        "NT_CONTAINER_MARKER",
        if container { &exists } else { &missing },
    )
    .env(
        "NT_SESSION_DIR",
        if graphical {
            fixtures().join("sessions")
        } else {
            PathBuf::from("/nonexistent/sessions")
        },
    )
    .env("NO_COLOR", "1")
    .args(["apply", "--only", "core", "--skip", "core"]);
    cmd
}

const ONE_OF_EACH: &str = "[extra]\nbrew = [\"ripgrep\"]\nmise = [\"go@latest\"]\n";

fn position(calls: &[String], prefix: &str) -> usize {
    calls
        .iter()
        .position(|c| c.starts_with(prefix))
        .unwrap_or_else(|| panic!("no call starting with {prefix:?} in {calls:#?}"))
}

fn count(calls: &[String], prefix: &str) -> usize {
    calls.iter().filter(|c| c.starts_with(prefix)).count()
}

#[test]
fn apply_queries_each_manager_then_installs_and_exits_zero() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .assert()
        .success()
        .stderr(predicate::str::contains("2 steps in"));

    let calls = sb.calls();
    for query in [
        "brew list --formula -1",
        "brew tap",
        "brew trust --json v1",
        "mise ls --global --json",
    ] {
        assert_eq!(count(&calls, query), 1, "{calls:#?}");
    }
    assert!(position(&calls, "brew list") < position(&calls, "brew install ripgrep"));
    assert!(position(&calls, "mise ls") < position(&calls, "mise use --global --yes go@latest"));
}

#[test]
fn a_converged_machine_has_nothing_to_do() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .env("FAKE_INSTALLED_BREW", "ripgrep")
        .env(
            "FAKE_INSTALLED_MISE_JSON",
            r#"{"go":[{"requested_version":"latest","installed":true}]}"#,
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to do."));

    let calls = sb.calls();
    assert_eq!(count(&calls, "brew install"), 0, "{calls:#?}");
    assert_eq!(count(&calls, "mise use"), 0, "{calls:#?}");
}

#[test]
fn a_failed_package_step_exits_one_and_skips_dotfiles_but_not_other_steps() {
    let sb = sandbox();
    sb.with_managers();
    let config = "[dotfiles]\nenabled = true\nrepo = \"https://example.invalid/dotfiles.git\"\n\
                  [extra]\nbrew = [\"ripgrep\"]\nmise = [\"go@latest\"]\n";

    apply(&sb, Host::Atomic, config)
        .env("FAKE_FAIL", "brew install")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("failed: brew install ripgrep"))
        .stderr(predicate::str::contains("simulated failure"))
        .stderr(predicate::str::contains("Skipping the dotfiles step"));

    let calls = sb.calls();
    assert_eq!(
        count(&calls, "mise use --global --yes go@latest"),
        1,
        "{calls:#?}"
    );
    assert_eq!(count(&calls, "chezmoi"), 0, "{calls:#?}");
}

#[test]
fn a_fresh_host_bootstraps_homebrew_then_mise_and_the_snapshot_sees_them() {
    let sb = sandbox();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .assert()
        .success()
        .stderr(predicate::str::contains("Bootstrapping package managers."));

    let calls = sb.calls();
    let installer = position(
        &calls,
        "curl -fsSL https://raw.githubusercontent.com/Homebrew/install",
    );
    let mise = position(&calls, "brew install mise");
    let query = position(&calls, "brew list --formula -1");
    let install = position(&calls, "brew install ripgrep");
    assert!(
        installer < mise && mise < query && query < install,
        "{calls:#?}"
    );
    assert!(sb.tools.join("brew").exists());
}

#[test]
fn a_failed_installer_download_ends_the_run_before_any_query() {
    let sb = sandbox();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .env("FAKE_FAIL", "curl")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("bootstrap failed at"));

    let calls = sb.calls();
    assert_eq!(count(&calls, "brew"), 0, "{calls:#?}");
    assert_eq!(count(&calls, "mise"), 0, "{calls:#?}");
}

#[test]
fn strict_exits_two_when_an_extra_has_no_manager_here() {
    let sb = sandbox();
    sb.with_managers();
    let config = "[extra]\ndnf = [\"some-kernel-tool\"]\n";

    apply(&sb, Host::Atomic, config).assert().success();
    apply(&sb, Host::Atomic, config)
        .arg("--strict")
        .assert()
        .code(2)
        .stdout(predicate::str::contains("some-kernel-tool"));
}

#[test]
fn sudo_is_primed_once_when_a_step_is_privileged() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Container, "[extra]\ndnf = [\"xdotool\"]\n")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Some steps need elevated privileges.",
        ));

    let calls = sb.calls();
    assert_eq!(count(&calls, "sudo -v"), 1, "{calls:#?}");
    assert_eq!(
        count(&calls, "sudo dnf install -y xdotool"),
        1,
        "{calls:#?}"
    );
    assert!(position(&calls, "sudo -v") < position(&calls, "sudo dnf install"));
}

#[test]
fn sudo_is_not_primed_when_a_credential_is_already_cached() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Container, "[extra]\ndnf = [\"xdotool\"]\n")
        .env("FAKE_SUDO_CACHED", "1")
        .assert()
        .success()
        .stderr(predicate::str::contains("elevated privileges").not());

    let calls = sb.calls();
    assert_eq!(count(&calls, "sudo -v"), 0, "{calls:#?}");
    assert_eq!(
        count(&calls, "sudo dnf install -y xdotool"),
        1,
        "{calls:#?}"
    );
}

#[test]
fn sudo_is_never_consulted_when_nothing_is_privileged() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, ONE_OF_EACH).assert().success();

    let calls = sb.calls();
    assert_eq!(count(&calls, "sudo"), 0, "{calls:#?}");
}

#[test]
fn a_sudo_using_dotfiles_script_is_primed_for_before_the_bootstrap() {
    // The one prompt must cover the whole run: a run script that needs
    // sudo is known before the bootstrap starts, so it is asked for then.
    let sb = sandbox();
    let source = sb.home.join(".local/share/chezmoi");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("run_once_cert.sh"), "sudo cp a /etc/b\n").unwrap();
    let config = "[dotfiles]\nenabled = true\nrepo = \"https://example.invalid/d.git\"\n\
                  [extra]\nbrew = [\"ripgrep\"]\n";

    // chezmoi is not faked: the run fails at that step, which is fine here.
    apply(&sb, Host::Atomic, config).assert().failure();

    let calls = sb.calls();
    assert_eq!(count(&calls, "sudo -v"), 1, "{calls:#?}");
    assert!(
        position(&calls, "sudo -v") < position(&calls, "curl"),
        "{calls:#?}"
    );
}

#[test]
fn upgrade_issues_upgrade_commands_for_what_is_installed() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, "[extra]\nbrew = [\"ripgrep\"]\n")
        .env("FAKE_INSTALLED_BREW", "ripgrep")
        .arg("--upgrade")
        .assert()
        .success();

    let calls = sb.calls();
    assert_eq!(count(&calls, "brew upgrade ripgrep"), 1, "{calls:#?}");
    assert_eq!(count(&calls, "brew install"), 0, "{calls:#?}");
}

#[test]
fn quiet_is_silent_on_success_and_reports_only_failures() {
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .arg("-q")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());

    let sb = sandbox();
    sb.with_managers();
    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .arg("-q")
        .env("FAKE_FAIL", "brew install")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("1 step failed"))
        .stderr(predicate::str::contains("steps in").not());
}

#[test]
fn json_mode_emits_one_document_with_the_plan_and_the_run() {
    let sb = sandbox();
    sb.with_managers();

    let out = apply(&sb, Host::Atomic, ONE_OF_EACH)
        .args(["--output", "json"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty())
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(v["plan"]["actions"].as_array().unwrap().len(), 2, "{v:#}");
    let steps = v["run"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2, "{v:#}");
    assert!(steps.iter().all(|s| s["success"] == true), "{v:#}");
}

#[test]
fn a_manager_that_cannot_list_its_packages_is_a_hard_error() {
    // Planning against a half-known world would install things twice.
    let sb = sandbox();
    sb.with_managers();

    apply(&sb, Host::Atomic, ONE_OF_EACH)
        .env("FAKE_FAIL", "brew list")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "failed to list packages installed by brew",
        ));

    let calls = sb.calls();
    assert_eq!(count(&calls, "brew install"), 0, "{calls:#?}");
}

#[test]
fn flatpak_adds_the_remote_before_installing_from_it() {
    let sb = sandbox();
    sb.with_managers();

    apply(
        &sb,
        Host::Atomic,
        "[extra]\nflatpak = [\"com.example.App\"]\n",
    )
    .assert()
    .success();

    let calls = sb.calls();
    let remote = position(&calls, "flatpak remote-add --user --if-not-exists flathub");
    let install = position(
        &calls,
        "flatpak install --user --noninteractive flathub com.example.App",
    );
    assert!(remote < install, "{calls:#?}");
}
