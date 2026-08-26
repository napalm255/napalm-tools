//! End-to-end tests driving the `nt` binary.
//!
//! Every test pins the machine-dependent inputs - hostname, os-release, the
//! ostree marker, the container marker, the session directory and the config
//! file - through the `NT_*` overrides, so the results do not depend on the
//! developer's own machine. The exception is the set of installed packages,
//! which the binary genuinely queries; assertions here are written to hold
//! whatever happens to be installed.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// Path to a fixture file.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A `nt` invocation with every platform input pinned.
fn nt(config: &Path, os_release: &str, atomic: bool, container: bool, graphical: bool) -> Command {
    let exists = fixture("os-release-bluefin");
    let missing = PathBuf::from("/nonexistent/marker");
    let mut cmd = Command::cargo_bin("nt").unwrap();
    cmd.env("NT_CONFIG", config)
        .env("NT_HOSTNAME", "testhost.example.com")
        .env("NT_OS_RELEASE", fixture(os_release))
        .env("NT_OSTREE_MARKER", if atomic { &exists } else { &missing })
        .env(
            "NT_CONTAINER_MARKER",
            if container { &exists } else { &missing },
        )
        .env(
            "NT_SESSION_DIR",
            if graphical {
                fixture("sessions")
            } else {
                PathBuf::from("/nonexistent/sessions")
            },
        )
        .env_remove("WSL_DISTRO_NAME")
        .env_remove("NT_FAKE_UID")
        .env("NO_COLOR", "1");
    cmd
}

/// Bluefin: atomic, graphical.
fn on_atomic(config: &Path) -> Command {
    nt(config, "os-release-bluefin", true, false, true)
}

/// Fedora Workstation: mutable, graphical.
fn on_workstation(config: &Path) -> Command {
    nt(config, "os-release-fedora", false, false, true)
}

/// The Fedora container image: mutable, headless.
fn in_container(config: &Path) -> Command {
    nt(config, "os-release-fedora", false, true, false)
}

/// A config naming a dnf-only extra package: the way a user actually reaches
/// the unavailable path on an atomic host.
const DNF_EXTRA: &str = "[extra]\ndnf = [\"some-kernel-tool\"]\n";

/// Write a config file into a temporary directory.
fn config_file(dir: &tempfile::TempDir, contents: &str) -> PathBuf {
    let p = dir.path().join("config.toml");
    std::fs::write(&p, contents).unwrap();
    p
}

fn stdout(assert: assert_cmd::assert::Assert) -> String {
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

#[test]
fn version_prints_a_bare_string_for_scripts() {
    let out = stdout(
        Command::cargo_bin("nt")
            .unwrap()
            .arg("version")
            .assert()
            .success(),
    );

    assert_eq!(out.trim(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_clap_version_flag_is_more_verbose_than_the_subcommand() {
    let out = stdout(
        Command::cargo_bin("nt")
            .unwrap()
            .arg("--version")
            .assert()
            .success(),
    );

    assert!(out.starts_with("nt "));
}

#[test]
fn bundles_lists_the_whole_catalog_and_detail_lists_packages() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let summary = stdout(on_atomic(&cfg).arg("bundles").assert().success());
    assert!(summary.contains("core") && summary.contains("java") && summary.contains("android"));
    assert!(!summary.contains("ripgrep"), "packages need --detail");

    let detail = stdout(
        on_atomic(&cfg)
            .args(["bundles", "--detail"])
            .assert()
            .success(),
    );
    assert!(detail.contains("ripgrep"), "{detail}");
    assert!(detail.contains("mise:java@corretto-21"), "{detail}");
}

#[test]
fn bundles_json_carries_packages_and_providers() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(
        on_atomic(&cfg)
            .args(["bundles", "--output", "json"])
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");

    assert_eq!(v["bundles"][0]["name"], "core");
    assert_eq!(
        v["bundles"][0]["packages"][0]["providers"][0]["manager"],
        "brew"
    );
}

#[test]
fn every_bundle_is_on_by_default_and_a_file_can_turn_one_off() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[bundles]\nandroid = false\n");

    let out = stdout(on_workstation(&cfg).arg("bundles").assert().success());
    let row = |name: &str| {
        out.lines()
            .find(|l| l.starts_with(name))
            .unwrap()
            .to_string()
    };

    assert!(row("android").contains("off"), "{}", row("android"));
    assert!(row("java").contains("on"), "{}", row("java"));
}

#[test]
fn graphical_bundles_are_not_applicable_in_a_container() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(in_container(&cfg).arg("bundles").assert().success());
    let row = |name: &str| {
        out.lines()
            .find(|l| l.starts_with(name))
            .unwrap()
            .to_string()
    };

    assert!(row("desktop").contains("n/a"), "{}", row("desktop"));
    assert!(row("fonts").contains("n/a"), "{}", row("fonts"));
    assert!(row("core").contains("on"), "{}", row("core"));
}

#[test]
fn skip_and_only_narrow_a_run() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(
        on_atomic(&cfg)
            .args([
                "bundles", "--only", "core", "--only", "rust", "--skip", "rust",
            ])
            .assert()
            .success(),
    );
    let row = |name: &str| {
        out.lines()
            .find(|l| l.starts_with(name))
            .unwrap()
            .to_string()
    };

    assert!(row("core").contains("on"), "{}", row("core"));
    assert!(row("rust").contains("off"), "{}", row("rust"));
    assert!(row("go").contains("off"), "{}", row("go"));
}

#[test]
fn an_unknown_bundle_name_is_refused_by_the_parser() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["bundles", "--skip", "nope"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("nope"));
}

#[test]
fn no_dnf_action_is_planned_on_an_atomic_host() {
    // Holds no matter what is installed: dnf is barred entirely.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["apply", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains("dnf install").not());
}

#[test]
fn a_package_with_no_user_space_provider_is_reported_on_an_atomic_host() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, DNF_EXTRA);

    on_atomic(&cfg)
        .args(["apply", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains("some-kernel-tool"))
        .stdout(predicates::str::contains("dnf is not available"));
}

#[test]
fn strict_mode_exits_two_when_a_package_cannot_be_provisioned() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, DNF_EXTRA);

    on_atomic(&cfg)
        .args(["apply", "--dry-run", "--strict"])
        .assert()
        .code(2);
}

#[test]
fn a_dry_run_on_a_fresh_host_plans_the_bootstrap_first() {
    // With PATH emptied of everything but the basics, neither brew nor mise
    // can be found, so the dry run must show how it would obtain them.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[dotfiles]\nenabled = false\n");

    let out = stdout(
        on_workstation(&cfg)
            .env("PATH", "/usr/bin:/bin")
            .env("NT_TOOL_DIRS", "")
            .env("HOME", dir.path())
            .args(["apply", "--dry-run", "--output", "json"])
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let kinds: Vec<&str> = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();

    assert_eq!(kinds[0], "bootstrap", "{kinds:?}");
    let bootstrap: Vec<&str> = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["kind"] == "bootstrap")
        .map(|a| a["command"].as_str().unwrap())
        .collect();
    assert!(
        bootstrap.iter().any(|c| c.contains("Homebrew/install")),
        "{bootstrap:?}"
    );
    assert!(
        bootstrap.iter().any(|c| c.contains("brew install mise")),
        "{bootstrap:?}"
    );
    // Once bootstrapped, brew and mise are assumed available, so the catalog
    // plans against them rather than reporting everything unavailable.
    assert!(
        kinds.contains(&"install"),
        "packages should be planned for install after bootstrap: {kinds:?}"
    );
    assert!(
        v["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "ripgrep" && p["state"] == "missing"),
        "{}",
        v["packages"]
    );
}

#[test]
fn a_dry_run_json_names_the_platform() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(
        in_container(&cfg)
            .args(["apply", "--dry-run", "--output", "json"])
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(v["platform"]["container"], true);
    assert_eq!(v["platform"]["graphical"], false);
    assert!(
        v["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "desktop")
    );
}

#[test]
fn apply_refuses_to_run_as_root() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_workstation(&cfg)
        .env("NT_FAKE_UID", "0")
        .args(["apply", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("root"));
}

#[test]
fn later_host_tables_win_over_earlier_ones() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(
        &dir,
        r#"
[host."*"]
bundles = { aws = true }

[host."*.example.com"]
bundles = { aws = false }
"#,
    );

    let out = stdout(on_atomic(&cfg).arg("bundles").assert().success());
    let aws_line = out.lines().find(|l| l.starts_with("aws")).unwrap();

    assert!(
        aws_line.contains("off"),
        "later table should win: {aws_line}"
    );
}

#[test]
fn config_show_reports_the_resolved_state_and_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(
        &dir,
        "[shell]\nprompt = \"oh-my-posh\"\n[options]\nupgrade = true\n",
    );

    on_atomic(&cfg)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicates::str::contains("prompt:   oh-my-posh"))
        .stdout(predicates::str::contains("upgrade:  true"))
        .stdout(predicates::str::contains("atomic"));
}

#[test]
fn config_path_honours_the_flag_and_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicates::str::contains(cfg.to_str().unwrap()));

    on_atomic(&cfg)
        .args(["config", "path", "--config", "/elsewhere/nt.toml"])
        .assert()
        .success()
        .stdout(predicates::str::contains("/elsewhere/nt.toml"));
}

#[test]
fn a_malformed_config_file_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[bundles\ncore = true");

    on_atomic(&cfg)
        .arg("bundles")
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml"));
}

#[test]
fn an_extra_shaped_like_a_flag_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[extra]\nbrew = [\"--force\"]\n");

    on_atomic(&cfg)
        .arg("bundles")
        .assert()
        .failure()
        .stderr(predicates::str::contains("--force"));
}

#[test]
fn shell_init_follows_the_configured_prompt() {
    let dir = tempfile::tempdir().unwrap();

    let cfg = config_file(&dir, "");
    on_atomic(&cfg)
        .args(["shell-init", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("starship init bash"));

    let cfg = config_file(&dir, "[shell]\nprompt = \"powerbash\"\n");
    on_atomic(&cfg)
        .args(["shell-init", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("powerbash.sh"));
    on_atomic(&cfg)
        .args(["shell-init", "zsh"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("bash"));
}

#[test]
fn completions_generate_for_each_shell() {
    for shell in ["bash", "zsh", "fish"] {
        let out = stdout(
            Command::cargo_bin("nt")
                .unwrap()
                .args(["completions", shell])
                .assert()
                .success(),
        );
        assert!(out.contains("nt"), "{shell}: {out}");
        assert!(out.contains("skip"), "{shell} should complete --skip");
    }
}

#[test]
fn json_output_is_a_single_parseable_document() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    for args in [
        vec!["bundles"],
        vec!["config", "show"],
        vec!["apply", "--dry-run"],
        vec!["status"],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let out = stdout(on_atomic(&cfg).args(&argv).assert().success());
        serde_json::from_str::<serde_json::Value>(&out)
            .unwrap_or_else(|e| panic!("{args:?}: {e}\n{out}"));
    }
}

#[test]
fn status_reports_desired_versus_installed_per_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(on_atomic(&cfg).arg("status").assert().success());

    assert!(out.contains("core"), "{out}");
    assert!(out.contains("packages present"), "{out}");
    assert!(!out.contains("explicitly installed"), "{out}");

    let json = stdout(
        on_atomic(&cfg)
            .args(["status", "--output", "json"])
            .assert()
            .success(),
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["bundles"][0]["name"], "core");
    assert!(v["totals"]["present"].is_number());
}

#[test]
fn stdout_carries_no_escapes_when_redirected() {
    // The answer goes to a pipe here whatever the terminal on stderr does.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = stdout(
        on_atomic(&cfg)
            .env_remove("NO_COLOR")
            .arg("bundles")
            .assert()
            .success(),
    );

    assert!(
        !out.contains('\u{1b}'),
        "escapes in redirected stdout: {out:?}"
    );
}

#[test]
fn a_dry_run_never_asks_for_a_password() {
    // `sudo` is not on this PATH at all, so any attempt to prime it would be
    // a hard failure rather than a hidden prompt.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[dotfiles]\nenabled = false\n");

    on_workstation(&cfg)
        .env("PATH", "/nonexistent")
        .env("NT_TOOL_DIRS", "")
        .env("HOME", dir.path())
        .args(["apply", "--dry-run"])
        .assert()
        .success();
}

#[test]
fn verbosity_is_accepted_without_changing_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let quiet = stdout(on_atomic(&cfg).arg("bundles").assert().success());
    let loud = stdout(on_atomic(&cfg).args(["bundles", "-vv"]).assert().success());

    assert_eq!(quiet, loud);
}
