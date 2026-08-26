//! End-to-end tests driving the `nt` binary.
//!
//! Every test pins the machine-dependent inputs — hostname, os-release, the
//! ostree marker and the config file — through the `NT_*` overrides, so the
//! results do not depend on the developer's own machine. The exception is the
//! set of installed packages, which the binary genuinely queries; assertions
//! here are written to hold whatever happens to be installed.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// Path to a fixture file.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A `nt` invocation with the environment pinned to an atomic Fedora host.
///
/// The ostree marker points at a file that certainly exists, which is what
/// makes the host read as atomic.
fn on_atomic(config: &Path) -> Command {
    let mut cmd = Command::cargo_bin("nt").unwrap();
    cmd.env("NT_CONFIG", config)
        .env("NT_HOSTNAME", "testhost.example.com")
        .env("NT_OS_RELEASE", fixture("os-release-bluefin"))
        .env("NT_OSTREE_MARKER", fixture("os-release-bluefin"))
        .env_remove("WSL_DISTRO_NAME");
    cmd
}

/// A `nt` invocation pinned to a traditional, mutable Fedora host.
fn on_traditional(config: &Path) -> Command {
    let mut cmd = Command::cargo_bin("nt").unwrap();
    cmd.env("NT_CONFIG", config)
        .env("NT_HOSTNAME", "testhost.example.com")
        .env("NT_OS_RELEASE", fixture("os-release-fedora"))
        .env("NT_OSTREE_MARKER", "/nonexistent/ostree-booted")
        .env_remove("WSL_DISTRO_NAME");
    cmd
}

/// A config naming a dnf-only extra package.
///
/// Every catalog package resolves through Homebrew, so `[extra]` is the only
/// way a user actually reaches dnf - which is what "last resort" means in
/// practice.
const DNF_EXTRA: &str = "[extra]\ndnf = [\"some-kernel-tool\"]\n";

/// Write a config file into a temporary directory.
fn config_file(dir: &tempfile::TempDir, contents: &str) -> PathBuf {
    let p = dir.path().join("config.toml");
    std::fs::write(&p, contents).unwrap();
    p
}

#[test]
fn version_prints_a_bare_string_for_scripts() {
    let out = Command::cargo_bin("nt")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();

    assert_eq!(
        text.trim(),
        env!("CARGO_PKG_VERSION"),
        "`nt version` must print the version alone so it can be piped"
    );
}

#[test]
fn the_clap_version_flag_is_more_verbose_than_the_subcommand() {
    let out = Command::cargo_bin("nt")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(String::from_utf8(out).unwrap().starts_with("nt "));
}

#[test]
fn bundles_lists_the_whole_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .arg("bundles")
        .assert()
        .success()
        .stdout(predicates::str::contains("core"))
        .stdout(predicates::str::contains("security"))
        .stdout(predicates::str::contains("node-runtime"))
        .stdout(predicates::str::contains("desktop"));
}

#[test]
fn no_dnf_action_is_planned_on_an_atomic_host() {
    // Holds no matter what is installed: dnf is barred entirely.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["apply", "--dry-run", "--desktop"])
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
fn strict_mode_succeeds_when_everything_can_be_provisioned() {
    let dir = tempfile::tempdir().unwrap();
    // desktop off, so the dnf-only package is never considered.
    let cfg = config_file(&dir, "[bundles]\ndesktop = false\n");

    on_atomic(&cfg)
        .args(["apply", "--dry-run", "--strict"])
        .assert()
        .success();
}

#[test]
fn the_same_package_is_planned_via_dnf_on_a_traditional_host() {
    // The mirror of the atomic case: identical config and catalog, and the
    // only difference is the platform.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_traditional(&cfg)
        .args(["apply", "--dry-run", "--desktop"])
        .assert()
        .success()
        .stdout(predicates::str::contains("no user-space provider").not());
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

    let out = on_atomic(&cfg).arg("bundles").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let aws_line = stdout.lines().find(|l| l.starts_with("aws")).unwrap();

    assert!(
        aws_line.contains("off"),
        "later table should win: {aws_line}"
    );
}

#[test]
fn a_command_line_flag_overrides_every_host_table() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[host.\"*\"]\nbundles = { aws = false }\n");

    let out = on_atomic(&cfg)
        .args(["bundles", "--aws"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let aws_line = stdout.lines().find(|l| l.starts_with("aws")).unwrap();

    assert!(aws_line.contains("on"), "flag should win: {aws_line}");
}

#[test]
fn a_typo_in_a_bundle_name_is_rejected_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[bundles]\ndevv = true\n");

    on_atomic(&cfg)
        .arg("bundles")
        .assert()
        .failure()
        .stderr(predicates::str::contains("devv"));
}

#[test]
fn config_path_reports_the_file_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicates::str::contains("config.toml"));
}

#[test]
fn config_show_reflects_the_detected_platform() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicates::str::contains("atomic=true"));

    on_traditional(&cfg)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicates::str::contains("atomic=false"));
}

#[test]
fn a_missing_config_file_is_not_an_error() {
    let mut cmd = Command::cargo_bin("nt").unwrap();
    cmd.env("NT_CONFIG", "/nonexistent/napalm-tools/config.toml")
        .env("NT_HOSTNAME", "testhost")
        .arg("bundles")
        .assert()
        .success();
}

#[test]
fn a_malformed_config_file_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[bundles\ncore = true");

    on_atomic(&cfg)
        .arg("bundles")
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml"));
}

#[test]
fn completions_are_generated_for_every_supported_shell() {
    for shell in ["bash", "zsh", "fish"] {
        Command::cargo_bin("nt")
            .unwrap()
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicates::str::contains("nt"));
    }
}

#[test]
fn generated_bash_completions_are_valid_bash() {
    let out = Command::cargo_bin("nt")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("nt.bash");
    std::fs::write(&script, out).unwrap();

    Command::new("bash")
        .arg("-n")
        .arg(&script)
        .assert()
        .success();
}

#[test]
fn generated_completions_mention_the_bundle_flags() {
    // Proof the parity guarantee reaches all the way to shell completion.
    let out = Command::cargo_bin("nt")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();

    assert!(text.contains("--no-node-runtime"), "missing generated flag");
}

#[test]
fn an_unknown_subcommand_fails_with_guidance() {
    Command::cargo_bin("nt")
        .unwrap()
        .arg("frobnicate")
        .assert()
        .failure()
        .stderr(
            predicates::str::contains("unrecognized").or(predicates::str::contains("unexpected")),
        );
}

#[test]
fn bare_nt_shows_help_rather_than_doing_anything() {
    Command::cargo_bin("nt")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicates::str::contains("Usage"));
}

// --- output formats ---------------------------------------------------------

/// Parse a command's stdout as JSON, failing loudly if anything else leaked in.
fn stdout_json(out: &[u8]) -> serde_json::Value {
    let text = String::from_utf8(out.to_vec()).unwrap();
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("stdout was not pure JSON ({e}):\n{text}"))
}

#[test]
fn bundles_emits_valid_json() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = on_atomic(&cfg)
        .args(["bundles", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v = stdout_json(&out);
    let names: Vec<&str> = v["bundles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"core"), "got {names:?}");
    assert_eq!(v["bundles"][0]["enabled"], true);
}

#[test]
fn a_dry_run_emits_valid_json_with_the_commands_it_would_run() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, DNF_EXTRA);

    let out = on_atomic(&cfg)
        .args(["apply", "--dry-run", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v = stdout_json(&out);
    assert_eq!(v["dry_run"], true);
    assert!(v["actions"].is_array());
    // The unavailable package is machine-readable too, not just prose.
    let unavailable: Vec<&str> = v["unavailable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["package"].as_str().unwrap())
        .collect();
    assert!(
        unavailable.contains(&"some-kernel-tool"),
        "got {unavailable:?}"
    );
}

#[test]
fn config_show_emits_valid_json_reporting_the_platform() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = on_atomic(&cfg)
        .args(["config", "show", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(stdout_json(&out)["platform"]["atomic"], true);
}

#[test]
fn json_output_carries_no_ansi_escapes() {
    // Otherwise redirecting to a file produces something no parser accepts.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = on_atomic(&cfg)
        .args(["bundles", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(
        !String::from_utf8(out).unwrap().contains('\u{1b}'),
        "escape sequences leaked into stdout"
    );
}

#[test]
fn diagnostics_stay_off_stdout() {
    // The whole point of the channel split: stdout is the answer, nothing else.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let assert = on_atomic(&cfg)
        .args(["apply", "--dry-run", "--desktop", "--output", "json", "-v"])
        .assert()
        .success();

    stdout_json(&assert.get_output().stdout);
}

#[test]
fn plain_output_is_requestable_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_atomic(&cfg)
        .args(["bundles", "--output", "plain"])
        .assert()
        .success()
        .stdout(predicates::str::contains("core"));
}

#[test]
fn verbosity_is_accepted_without_changing_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let quiet_run = on_atomic(&cfg)
        .args(["apply", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let loud_run = on_atomic(&cfg)
        .args(["apply", "--dry-run", "-vv"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(
        String::from_utf8(quiet_run).unwrap(),
        String::from_utf8(loud_run).unwrap(),
        "verbosity should change diagnostics, not the answer"
    );
}

#[test]
fn status_emits_valid_json() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = on_atomic(&cfg)
        .args(["status", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(stdout_json(&out)["actions"].is_array());
}

#[test]
fn quiet_is_silent_on_success() {
    // Silence means it worked; anything printed would defeat the flag.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let assert = on_atomic(&cfg)
        .args(["apply", "--dry-run", "-q"])
        .assert()
        .success();
    let out = assert.get_output();

    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn quiet_still_reports_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "[bundles]\ndevv = true\n");

    on_atomic(&cfg)
        .args(["apply", "--dry-run", "-q"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("devv"));
}

#[test]
fn quiet_is_refused_where_it_would_only_discard_the_answer() {
    // Better to reject the flag than accept it and print nothing.
    for argv in [
        vec!["version", "-q"],
        vec!["bundles", "-q"],
        vec!["completions", "bash", "-q"],
    ] {
        Command::cargo_bin("nt")
            .unwrap()
            .args(&argv)
            .assert()
            .failure()
            .stderr(predicates::str::contains("unexpected argument"));
    }
}

#[test]
fn version_refuses_flags_that_could_only_contradict_it() {
    for argv in [
        vec!["version", "--output", "json"],
        vec!["version", "--config", "/tmp/x.toml"],
    ] {
        Command::cargo_bin("nt")
            .unwrap()
            .args(&argv)
            .assert()
            .failure();
    }
}

// --- privileges -------------------------------------------------------------

#[test]
fn dnf_actions_escalate_privileges() {
    // Without sudo the command cannot succeed: dnf refuses to run as a
    // normal user, so the old unprivileged form was dead on arrival.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, DNF_EXTRA);

    on_traditional(&cfg)
        .args(["apply", "--dry-run"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "sudo dnf install -y some-kernel-tool",
        ));
}

#[test]
fn privileged_steps_are_visible_before_the_run_starts() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, DNF_EXTRA);

    let out = on_traditional(&cfg)
        .args(["apply", "--dry-run", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v = stdout_json(&out);
    let needs_password = v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["privileged"] == true);

    assert!(
        needs_password,
        "a dnf run should declare that it needs a password"
    );
}

#[test]
fn no_privileged_action_is_planned_on_an_atomic_host() {
    // dnf is refused there entirely, so nothing should ask for a password.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    let out = on_atomic(&cfg)
        .args(["apply", "--dry-run", "--desktop", "--output", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v = stdout_json(&out);
    for action in v["actions"].as_array().unwrap() {
        assert_eq!(
            action["privileged"], false,
            "unexpected privileged action: {action}"
        );
    }
}

#[test]
fn a_dry_run_never_asks_for_a_password() {
    // Nothing executes, so nothing should prompt or prime.
    let dir = tempfile::tempdir().unwrap();
    let cfg = config_file(&dir, "");

    on_traditional(&cfg)
        .args(["apply", "--dry-run", "--desktop"])
        .assert()
        .success()
        .stderr(predicates::str::contains("privileges").not());
}
