//! End-to-end tests of `nt self check` and `nt self update`.
//!
//! The binary under test is a *copy* in a writable sandbox, so an update
//! really does replace a file on disk and the assertion is on the file
//! rather than on a mock. `target/debug/nt` itself is correctly refused as
//! a cargo build tree, and must never be replaced by a test run.
//!
//! The releases API and the asset downloads are served by the fake `curl`
//! in `tests/fixtures/fake-bin`, keyed on the real URLs - there is no
//! override that repoints `nt` at another host, because a tool that runs
//! `curl` on a timer should not carry one.

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The architecture the release archive is named for.
const ARCH: &str = std::env::consts::ARCH;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Where sandboxes live: under `target/`, so the installed copy can be a
/// hard link to the test binary. Copying it instead races with the other
/// tests - a writable descriptor open in one thread while another forks
/// makes the exec fail with ETXTBSY - and a hard link never opens the file
/// for writing at all. A link also keeps `target/debug/nt` safe: the update
/// replaces a directory entry, not the inode.
fn sandbox_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-sandboxes");
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Put a runnable `nt` at `dest`, linked rather than copied.
fn link_nt(dest: &Path) {
    std::fs::hard_link(assert_cmd::cargo::cargo_bin("nt"), dest).unwrap();
}

/// A throwaway machine: an installed `nt`, prepared assets, a cache.
struct Sandbox {
    _dir: tempfile::TempDir,
    home: PathBuf,
    /// Where the copy of `nt` under test lives.
    bin: PathBuf,
    /// Where release archives are served from.
    assets: PathBuf,
    cache: PathBuf,
    log: PathBuf,
}

fn sandbox() -> Sandbox {
    let dir = tempfile::TempDir::new_in(sandbox_root()).unwrap();
    let home = dir.path().join("home");
    let bin = home.join(".local/bin");
    let assets = dir.path().join("assets");
    for d in [&home, &bin, &assets] {
        std::fs::create_dir_all(d).unwrap();
    }
    // The update replaces this directory entry, never the test binary.
    link_nt(&bin.join("nt"));
    Sandbox {
        cache: dir.path().join("update-check.json"),
        log: dir.path().join("calls.log"),
        _dir: dir,
        home,
        bin,
        assets,
    }
}

impl Sandbox {
    /// The installed binary the tests drive.
    fn nt(&self) -> PathBuf {
        self.bin.join("nt")
    }

    fn inode(&self) -> u64 {
        std::fs::metadata(self.nt()).unwrap().ino()
    }
}

/// Build a release archive laid out exactly as `just release-assets` does,
/// whose `nt` is a script so a test can make it misbehave.
fn release_archive(sb: &Sandbox, version: &str, body: &str) {
    let name = format!("nt-v{version}-{ARCH}-unknown-linux-gnu");
    let staging = sb.assets.join("staging");
    let inner = staging.join(&name);
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("nt"), body).unwrap();
    std::fs::set_permissions(inner.join("nt"), std::fs::Permissions::from_mode(0o755)).unwrap();
    for extra in ["LICENSE", "README.md", "nt.bash", "nt.zsh", "nt.fish"] {
        std::fs::write(inner.join(extra), "x").unwrap();
    }
    let tarball = format!("{name}.tar.gz");
    assert!(
        Command::new("tar")
            .args(["-C", &staging.to_string_lossy(), "-czf", &tarball, &name])
            .current_dir(&sb.assets)
            .status()
            .unwrap()
            .success()
    );
    let sums = Command::new("sha256sum")
        .arg(&tarball)
        .current_dir(&sb.assets)
        .output()
        .unwrap();
    std::fs::write(sb.assets.join(format!("{tarball}.sha256")), &sums.stdout).unwrap();
    std::fs::remove_dir_all(&staging).unwrap();
}

/// A stand-in `nt` that answers `version`.
fn stub_binary(version: &str) -> String {
    format!("#!/usr/bin/env bash\n[[ \"$1\" == version ]] && echo {version}\n")
}

/// The release document the fake curl returns for the API.
fn release_json(version: &str) -> String {
    let name = format!("nt-v{version}-{ARCH}-unknown-linux-gnu.tar.gz");
    format!(
        r#"{{"tag_name":"v{version}","assets":[
          {{"name":"{name}","browser_download_url":"https://github.com/napalm255/napalm-tools/releases/download/v{version}/{name}"}},
          {{"name":"{name}.sha256","browser_download_url":"https://github.com/napalm255/napalm-tools/releases/download/v{version}/{name}.sha256"}}]}}"#
    )
}

/// Run the installed `nt` with every machine input pinned.
fn nt(sb: &Sandbox, args: &[&str]) -> Command {
    let mut cmd = Command::new(sb.nt());
    cmd.env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fixtures().join("fake-bin").display()),
        )
        .env("HOME", &sb.home)
        .env("NT_UPDATE_CACHE", &sb.cache)
        .env("FAKE_LOG", &sb.log)
        .env("FAKE_ASSET_DIR", &sb.assets)
        .env("NO_COLOR", "1")
        .args(args);
    // Keep the copy's coverage in the same profile as the run that spawned it.
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile);
    }
    cmd
}

/// The version this test binary reports, which is what a release must beat.
fn current() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A version one minor ahead of the running one.
fn newer() -> String {
    let current = current();
    let mut parts = current.split('.');
    let major: u64 = parts.next().unwrap().parse().unwrap();
    let minor: u64 = parts.next().unwrap().parse().unwrap();
    format!("{major}.{}.0", minor + 1)
}

#[test]
fn self_check_reports_a_newer_release_and_leaves_the_binary_alone() {
    let sb = sandbox();
    let before = sb.inode();

    nt(&sb, &["self", "check"])
        .env("FAKE_RELEASE_JSON", release_json(&newer()))
        .assert()
        .success()
        .stdout(predicate::str::contains(newer()))
        .stdout(predicate::str::contains("nt self update"));

    assert_eq!(sb.inode(), before, "check must not touch the binary");
}

#[test]
fn self_check_says_nothing_is_newer_when_the_release_matches() {
    let sb = sandbox();

    nt(&sb, &["self", "check"])
        .env("FAKE_RELEASE_JSON", release_json(&current()))
        .assert()
        .success()
        .stdout(predicate::str::contains("already the latest"));
}

#[test]
fn self_check_records_its_answer_so_the_automatic_check_does_not_ask_again() {
    let sb = sandbox();

    nt(&sb, &["self", "check"])
        .env("FAKE_RELEASE_JSON", release_json(&newer()))
        .assert()
        .success();

    let cached = std::fs::read_to_string(&sb.cache).unwrap();
    assert!(cached.contains(&newer()), "{cached}");
}

#[test]
fn self_update_downloads_verifies_and_replaces_the_running_binary() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .success()
        .stdout(predicate::str::contains(current()))
        .stdout(predicate::str::contains(&version));

    // Three ways, so this cannot pass by accident.
    assert!(std::fs::read_to_string(sb.nt()).unwrap().contains(&version));
    Command::new(sb.nt())
        .arg("version")
        .assert()
        .success()
        .stdout(format!("{version}\n"));
    assert_ne!(
        sb.inode(),
        before,
        "the file was renamed over, not rewritten"
    );
}

#[test]
fn self_update_clears_the_cache_so_no_notice_follows_the_update() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));
    std::fs::write(&sb.cache, r#"{"checked_at":1,"latest":"9.9.9"}"#).unwrap();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .success();

    assert!(
        !sb.cache.exists(),
        "a stale cache would announce a phantom release"
    );
}

#[test]
fn self_update_is_a_no_op_and_exits_zero_when_already_current() {
    let sb = sandbox();
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&current()))
        .assert()
        .success()
        .stdout(predicate::str::contains("already the latest"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn self_update_does_nothing_when_the_release_is_older_than_this_binary() {
    let sb = sandbox();
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json("0.0.1"))
        .assert()
        .success()
        .stdout(predicate::str::contains("already the latest"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn self_update_installs_a_named_release_even_when_it_is_older() {
    // The way back from a bad release.
    let sb = sandbox();
    release_archive(&sb, "0.0.1", &stub_binary("0.0.1"));

    nt(&sb, &["self", "update", "--version", "0.0.1"])
        .env("FAKE_RELEASE_JSON", release_json("0.0.1"))
        .assert()
        .success();

    Command::new(sb.nt())
        .arg("version")
        .assert()
        .stdout("0.0.1\n");
}

#[test]
fn self_update_force_reinstalls_the_version_already_running() {
    let sb = sandbox();
    release_archive(&sb, &current(), &stub_binary(&current()));

    nt(&sb, &["self", "update", "--force"])
        .env("FAKE_RELEASE_JSON", release_json(&current()))
        .assert()
        .success();

    assert!(std::fs::read_to_string(sb.nt()).unwrap().contains("bash"));
}

#[test]
fn self_update_dry_run_says_what_it_would_do_and_downloads_nothing() {
    let sb = sandbox();
    let version = newer();
    let before = sb.inode();

    nt(&sb, &["self", "update", "--dry-run"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .success()
        .stdout(predicate::str::contains(&version));

    assert_eq!(sb.inode(), before);
    let calls = std::fs::read_to_string(&sb.log).unwrap_or_default();
    assert!(!calls.contains("releases/download"), "{calls}");
}

#[test]
fn a_checksum_mismatch_refuses_to_install_and_leaves_the_old_binary() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));
    // Corrupt the archive after its checksum was recorded.
    let tarball = sb
        .assets
        .join(format!("nt-v{version}-{ARCH}-unknown-linux-gnu.tar.gz"));
    std::fs::write(&tarball, b"not the archive you verified").unwrap();
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("checksum verification failed"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn an_archive_without_nt_inside_is_refused_before_anything_is_replaced() {
    let sb = sandbox();
    let version = newer();
    let name = format!("nt-v{version}-{ARCH}-unknown-linux-gnu");
    // An archive with the right name and the wrong contents.
    let staging = sb.assets.join("staging");
    std::fs::create_dir_all(staging.join("somewhere-else")).unwrap();
    std::fs::write(staging.join("somewhere-else/other"), "x").unwrap();
    Command::new("tar")
        .args([
            "-C",
            &staging.to_string_lossy(),
            "-czf",
            &format!("{name}.tar.gz"),
            "somewhere-else",
        ])
        .current_dir(&sb.assets)
        .status()
        .unwrap();
    let sums = Command::new("sha256sum")
        .arg(format!("{name}.tar.gz"))
        .current_dir(&sb.assets)
        .output()
        .unwrap();
    std::fs::write(
        sb.assets.join(format!("{name}.tar.gz.sha256")),
        &sums.stdout,
    )
    .unwrap();
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("does not contain"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn a_downloaded_binary_that_will_not_run_is_refused() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, "#!/usr/bin/env bash\nexit 3\n");
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("does not run"));

    assert_eq!(sb.inode(), before, "the old binary must survive");
}

#[test]
fn a_downloaded_binary_reporting_the_wrong_version_is_refused() {
    let sb = sandbox();
    let version = newer();
    // Right archive name, wrong binary inside: a badly built release.
    release_archive(&sb, &version, &stub_binary("9.9.9"));
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .code(1)
        .stderr(predicate::str::contains("9.9.9"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn a_failed_download_names_the_url_and_changes_nothing() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));
    let before = sb.inode();

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .env("FAKE_CURL_STATUS", "7")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("api.github.com"));

    assert_eq!(sb.inode(), before);
}

#[test]
fn a_release_with_no_asset_for_this_target_names_the_asset_it_wanted() {
    let sb = sandbox();
    let version = newer();
    let body = format!(
        r#"{{"tag_name":"v{version}","assets":[{{"name":"nt-v{version}-sparc-unknown-linux-gnu.tar.gz","browser_download_url":"https://github.com/napalm255/napalm-tools/releases/download/v{version}/x.tar.gz"}}]}}"#
    );

    nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", body)
        .assert()
        .code(1)
        .stderr(predicate::str::contains(ARCH))
        .stderr(predicate::str::contains("sparc"));
}

#[test]
fn malformed_release_metadata_is_reported_rather_than_parsed_as_a_version() {
    let sb = sandbox();

    nt(&sb, &["self", "check"])
        .env("FAKE_RELEASE_JSON", "{not json at all")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("could not read the release"));
}

#[test]
fn a_release_tag_that_is_not_a_version_is_refused() {
    let sb = sandbox();

    nt(&sb, &["self", "check"])
        .env("FAKE_RELEASE_JSON", r#"{"tag_name":"nightly","assets":[]}"#)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nightly"));
}

#[test]
fn self_update_refuses_a_binary_inside_a_cargo_target_directory() {
    // The binary cargo built, run in place: cargo owns it.
    let sb = sandbox();
    let target = sb.home.join("project/target/release");
    std::fs::create_dir_all(&target).unwrap();
    link_nt(&target.join("nt"));

    let mut cmd = Command::new(target.join("nt"));
    cmd.env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fixtures().join("fake-bin").display()),
        )
        .env("HOME", &sb.home)
        .env("NT_UPDATE_CACHE", &sb.cache)
        .env("FAKE_LOG", &sb.log)
        .env("NO_COLOR", "1")
        .args(["self", "update"]);
    cmd.assert()
        .code(1)
        .stderr(predicate::str::contains("cargo build"));
}

#[test]
fn self_update_refuses_a_binary_it_cannot_replace() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));
    // A read-only directory: the rename could not land.
    std::fs::set_permissions(&sb.bin, std::fs::Permissions::from_mode(0o500)).unwrap();

    let assertion = nt(&sb, &["self", "update"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .code(1);

    std::fs::set_permissions(&sb.bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    assertion.stderr(predicate::str::contains("not writable"));
}

#[test]
fn self_update_output_json_is_one_document_naming_both_versions() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));

    let out = nt(&sb, &["self", "update", "--output", "json"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(v["current"], current());
    assert_eq!(v["latest"], version);
    assert_eq!(v["action"], "updated");
    assert_eq!(v["path"], sb.nt().to_string_lossy().as_ref());
}

#[test]
fn self_check_output_json_reports_what_it_would_do() {
    let sb = sandbox();

    let out = nt(&sb, &["self", "check", "--output", "json"])
        .env("FAKE_RELEASE_JSON", release_json(&newer()))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();

    assert_eq!(v["action"], "would-update");
    assert_eq!(v["latest"], newer());
}

#[test]
fn self_update_quiet_is_silent_on_success() {
    let sb = sandbox();
    let version = newer();
    release_archive(&sb, &version, &stub_binary(&version));

    nt(&sb, &["self", "update", "-q"])
        .env("FAKE_RELEASE_JSON", release_json(&version))
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    assert!(std::fs::read_to_string(sb.nt()).unwrap().contains(&version));
}
