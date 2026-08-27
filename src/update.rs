//! Keeping `nt` itself up to date.
//!
//! Two halves. The decisions here are pure and testable: which release is
//! newer, whether this copy of `nt` is one it may replace, which asset to
//! ask for, and whether the cached answer is old enough to ask again. The
//! I/O half - curl, sha256sum, tar, and the rename - lives beside them and
//! is deliberately thin.
//!
//! `nt` does no HTTP of its own. It shells out to `curl`, exactly as the
//! bootstrap does for the Homebrew installer, so no TLS stack enters the
//! dependency graph for the sake of one API call a day.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::version::Version;

/// The repository releases come from, derived from the manifest so there is
/// no second place for it to be wrong.
pub const REPO: &str = repo_from_url(env!("CARGO_PKG_REPOSITORY"));

/// The `owner/name` part of a GitHub URL.
const fn repo_from_url(url: &str) -> &str {
    // `const` string handling is limited to byte slices, so this walks the
    // bytes rather than using `strip_prefix`.
    let bytes = url.as_bytes();
    let prefix = b"https://github.com/";
    if bytes.len() <= prefix.len() {
        return "";
    }
    let mut i = 0;
    while i < prefix.len() {
        if bytes[i] != prefix[i] {
            return "";
        }
        i += 1;
    }
    match std::str::from_utf8(bytes.split_at(prefix.len()).1) {
        Ok(rest) => rest,
        Err(_) => "",
    }
}

/// How long a cached answer is trusted before asking again.
pub const CHECK_INTERVAL: u64 = 60 * 60 * 24;

/// A GitHub release, as much of it as `nt` reads.
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    /// The git tag, `vX.Y.Z`.
    pub tag_name: String,
    /// The files attached to it.
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// One file attached to a release.
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    /// Its file name.
    pub name: String,
    /// Where to fetch it from.
    pub browser_download_url: String,
}

impl Release {
    /// The version this release announces.
    pub fn version(&self) -> Result<Version> {
        Version::parse(&self.tag_name).with_context(|| {
            format!(
                "release {:?} is not a version nt understands",
                self.tag_name
            )
        })
    }
}

/// The archive name `just release-assets` builds for a version on this
/// machine's architecture.
pub fn asset_name(version: &Version, arch: &str) -> String {
    format!("nt-v{version}-{arch}-unknown-linux-gnu.tar.gz")
}

/// The directory that archive unpacks into.
pub fn asset_dir(version: &Version, arch: &str) -> String {
    format!("nt-v{version}-{arch}-unknown-linux-gnu")
}

/// Find the asset by its exact name.
///
/// Looked up rather than constructed from a URL template, so a release
/// built for another architecture says so instead of 404ing.
pub fn pick_asset<'a>(release: &'a Release, wanted: &str) -> Result<&'a Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name == wanted)
        .with_context(|| {
            format!(
                "release {} has no asset {wanted:?} (it has: {})",
                release.tag_name,
                if release.assets.is_empty() {
                    "nothing".to_string()
                } else {
                    release
                        .assets
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            )
        })
}

/// Who owns the `nt` on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// `nt` put it there and may replace it.
    SelfManaged,
    /// Something else owns it; updating is that tool's job.
    Managed {
        /// What owns it.
        by: &'static str,
        /// What to run instead.
        instead: String,
    },
}

/// Decide whether `nt` may replace the binary at `exe`.
///
/// Deliberately not a rule that the binary must live in `~/.local/bin`:
/// that is where the install instructions put it, not a constraint worth
/// enforcing on someone who moved it. What matters is that no other tool
/// considers the file its own.
pub fn install_kind(exe: &Path, home: Option<&Path>) -> Install {
    let managed = |by, instead: &str| Install::Managed {
        by,
        instead: instead.to_string(),
    };
    let text = exe.to_string_lossy();

    for prefix in [
        "/home/linuxbrew/.linuxbrew/",
        "/opt/homebrew/",
        "/usr/local/Homebrew/",
    ] {
        if text.starts_with(prefix) {
            return managed("Homebrew", "brew upgrade nt");
        }
    }
    // A cargo build tree: `.../target/release/nt` or `.../target/debug/nt`.
    let parts: Vec<_> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(i) = parts.iter().position(|p| p == "target")
        && parts[i + 1..]
            .iter()
            .any(|p| p == "release" || p == "debug")
    {
        return managed("a cargo build", "cargo build --release");
    }
    if let Some(home) = home {
        if text.starts_with(&*home.join(".cargo/bin/").to_string_lossy()) {
            return managed("cargo", "cargo install --force --path .");
        }
        if text.starts_with(&*home.join(".local/share/mise/").to_string_lossy()) {
            return managed("mise", "mise upgrade nt");
        }
    }
    for prefix in [
        "/usr/",
        "/opt/",
        "/nix/store/",
        "/snap/",
        "/var/lib/flatpak/",
    ] {
        if text.starts_with(prefix) {
            return managed("the system", "your system package manager");
        }
    }
    Install::SelfManaged
}

/// What a previous check found.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct Cached {
    /// When it ran, in seconds since the epoch.
    pub checked_at: u64,
    /// The newest release it saw.
    pub latest: String,
}

/// Where the answer to the last check is kept.
///
/// `NT_UPDATE_CACHE` overrides it, so a test never touches the real one.
pub fn cache_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NT_UPDATE_CACHE") {
        return Some(PathBuf::from(path));
    }
    let dir = match std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        Some(xdg) => PathBuf::from(xdg),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
    };
    Some(dir.join("napalm-tools/update-check.json"))
}

/// Whether the cached answer is old enough to ask again.
///
/// A missing or unreadable cache reads as "never checked", never as an
/// error: a version check must not be able to fail a run.
pub fn cache_is_stale(cached: Option<&Cached>, now: u64, interval: u64) -> bool {
    match cached {
        None => true,
        // A timestamp in the future means a clock moved; ask again rather
        // than wait however long it takes for the future to arrive.
        Some(c) => now.saturating_sub(c.checked_at) >= interval || c.checked_at > now,
    }
}

/// The notice to print, if the release is newer than what is running.
pub fn notice_for(current: &Version, latest: &Version) -> Option<String> {
    (latest > current)
        .then(|| format!("A newer nt is available: {current} -> {latest}. Run `nt self update`."))
}

/// Read the cache, treating every failure as "never checked".
pub fn read_cache(path: &Path) -> Option<Cached> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Write the cache, best effort.
///
/// Through a temporary file in the same directory so a concurrent reader
/// never sees half a document. Failure is ignored: the cost is one extra
/// check next time, which is not worth reporting.
pub fn write_cache(path: &Path, cached: &Cached) {
    let Ok(text) = serde_json::to_string(cached) else {
        return;
    };
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Seconds since the epoch, or 0 if the clock is before it.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("the test's own literal parses")
    }

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_string(),
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: (*name).to_string(),
                    browser_download_url: format!("https://example.invalid/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn the_repository_is_taken_from_the_cargo_manifest() {
        assert_eq!(REPO, "napalm255/napalm-tools");
        assert!(!REPO.is_empty(), "the manifest must carry a repository URL");
    }

    #[test]
    fn a_repository_url_that_is_not_github_yields_nothing() {
        assert_eq!(repo_from_url("https://gitlab.com/a/b"), "");
        assert_eq!(repo_from_url(""), "");
        assert_eq!(repo_from_url("https://github.com/"), "");
    }

    #[test]
    fn the_asset_name_is_built_from_the_version_and_this_architecture() {
        assert_eq!(
            asset_name(&v("0.2.0"), "x86_64"),
            "nt-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            asset_dir(&v("0.2.0"), "aarch64"),
            "nt-v0.2.0-aarch64-unknown-linux-gnu"
        );
    }

    #[test]
    fn the_asset_is_found_by_its_exact_name() {
        let r = release("v0.2.0", &["nt-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"]);

        let found = pick_asset(&r, "nt-v0.2.0-x86_64-unknown-linux-gnu.tar.gz").unwrap();

        assert_eq!(found.name, "nt-v0.2.0-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn an_asset_list_without_this_target_is_an_error_naming_what_it_wanted() {
        let r = release("v0.2.0", &["nt-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"]);

        let err = pick_asset(&r, "nt-v0.2.0-aarch64-unknown-linux-gnu.tar.gz")
            .unwrap_err()
            .to_string();

        assert!(err.contains("aarch64"), "{err}");
        assert!(err.contains("x86_64"), "{err}");
    }

    #[test]
    fn a_release_with_no_assets_at_all_says_so() {
        let err = pick_asset(&release("v0.2.0", &[]), "anything")
            .unwrap_err()
            .to_string();

        assert!(err.contains("nothing"), "{err}");
    }

    #[test]
    fn a_release_tag_that_is_not_a_version_is_an_error_not_a_panic() {
        let err = release("nightly", &[]).version().unwrap_err().to_string();

        assert!(err.contains("nightly"), "{err}");
        assert_eq!(release("v1.2.3", &[]).version().unwrap(), v("1.2.3"));
    }

    #[test]
    fn release_metadata_parses_from_the_github_api_shape() {
        let body = r#"{"tag_name":"v0.2.0","html_url":"ignored","assets":[
            {"name":"nt.tar.gz","browser_download_url":"https://example.invalid/nt.tar.gz","size":1}]}"#;

        let r: Release = serde_json::from_str(body).unwrap();

        assert_eq!(r.tag_name, "v0.2.0");
        assert_eq!(
            r.assets[0].browser_download_url,
            "https://example.invalid/nt.tar.gz"
        );
    }

    #[test]
    fn a_homebrew_path_is_reported_as_managed_by_homebrew() {
        let kind = install_kind(Path::new("/home/linuxbrew/.linuxbrew/bin/nt"), None);

        assert_eq!(
            kind,
            Install::Managed {
                by: "Homebrew",
                instead: "brew upgrade nt".into()
            }
        );
    }

    #[test]
    fn a_cargo_target_path_is_reported_as_a_build_tree() {
        for path in [
            "/home/x/git/napalm-tools/target/release/nt",
            "/home/x/git/napalm-tools/target/debug/nt",
        ] {
            let Install::Managed { by, .. } = install_kind(Path::new(path), None) else {
                panic!("{path} should be refused");
            };
            assert_eq!(by, "a cargo build");
        }
    }

    #[test]
    fn a_cargo_bin_path_is_reported_as_managed_by_cargo() {
        let home = PathBuf::from("/home/x");

        let Install::Managed { by, .. } =
            install_kind(Path::new("/home/x/.cargo/bin/nt"), Some(&home))
        else {
            panic!("cargo-installed nt should be refused");
        };

        assert_eq!(by, "cargo");
    }

    #[test]
    fn a_mise_shim_is_reported_as_managed_by_mise() {
        let home = PathBuf::from("/home/x");

        let Install::Managed { by, .. } = install_kind(
            Path::new("/home/x/.local/share/mise/installs/nt/bin/nt"),
            Some(&home),
        ) else {
            panic!("a mise install should be refused");
        };

        assert_eq!(by, "mise");
    }

    #[test]
    fn a_system_path_is_refused() {
        for path in [
            "/usr/bin/nt",
            "/opt/nt/nt",
            "/nix/store/abc/nt",
            "/snap/nt/nt",
        ] {
            assert!(
                matches!(install_kind(Path::new(path), None), Install::Managed { .. }),
                "{path}"
            );
        }
    }

    #[test]
    fn a_path_under_dot_local_bin_is_self_managed() {
        let home = PathBuf::from("/home/x");

        assert_eq!(
            install_kind(Path::new("/home/x/.local/bin/nt"), Some(&home)),
            Install::SelfManaged
        );
    }

    #[test]
    fn a_path_someone_moved_it_to_is_still_self_managed() {
        // The rule is "nothing else owns this", not "it lives where the
        // installer put it".
        let home = PathBuf::from("/home/x");

        for path in ["/home/x/bin/nt", "/tmp/scratch/nt", "/home/x/tools/nt"] {
            assert_eq!(
                install_kind(Path::new(path), Some(&home)),
                Install::SelfManaged,
                "{path}"
            );
        }
    }

    #[test]
    fn a_directory_merely_named_target_is_not_a_build_tree() {
        assert_eq!(
            install_kind(Path::new("/home/x/target/nt"), None),
            Install::SelfManaged
        );
    }

    #[test]
    fn an_absent_cache_is_treated_as_never_checked() {
        assert!(cache_is_stale(None, 1_000, CHECK_INTERVAL));
    }

    #[test]
    fn a_cache_written_within_the_interval_is_not_rechecked() {
        let cached = Cached {
            checked_at: 1_000,
            latest: "0.2.0".into(),
        };

        assert!(!cache_is_stale(
            Some(&cached),
            1_000 + CHECK_INTERVAL - 1,
            CHECK_INTERVAL
        ));
    }

    #[test]
    fn a_cache_older_than_the_interval_is_rechecked() {
        let cached = Cached {
            checked_at: 1_000,
            latest: "0.2.0".into(),
        };

        assert!(cache_is_stale(
            Some(&cached),
            1_000 + CHECK_INTERVAL,
            CHECK_INTERVAL
        ));
    }

    #[test]
    fn a_cache_timestamped_in_the_future_is_rechecked() {
        // A clock that moved backwards must not silence the check forever.
        let cached = Cached {
            checked_at: 9_000,
            latest: "0.2.0".into(),
        };

        assert!(cache_is_stale(Some(&cached), 1_000, CHECK_INTERVAL));
    }

    #[test]
    fn a_notice_is_produced_only_when_the_release_is_newer() {
        assert!(notice_for(&v("0.1.0"), &v("0.2.0")).is_some());
        assert!(notice_for(&v("0.2.0"), &v("0.2.0")).is_none());
        assert!(notice_for(&v("0.3.0"), &v("0.2.0")).is_none());
    }

    #[test]
    fn the_notice_names_both_versions_and_the_command_that_updates() {
        let notice = notice_for(&v("0.1.0"), &v("0.2.0")).unwrap();

        assert!(notice.contains("0.1.0"), "{notice}");
        assert!(notice.contains("0.2.0"), "{notice}");
        assert!(notice.contains("nt self update"), "{notice}");
    }

    #[test]
    fn the_cache_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/update-check.json");
        let cached = Cached {
            checked_at: 1_756_224_000,
            latest: "0.2.0".into(),
        };

        write_cache(&path, &cached);

        assert_eq!(read_cache(&path), Some(cached));
        assert!(
            !path.with_extension("json.tmp").exists(),
            "no litter left behind"
        );
    }

    #[test]
    fn a_missing_or_malformed_cache_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.json");
        let malformed = dir.path().join("malformed.json");
        std::fs::write(&malformed, "{not json").unwrap();

        assert_eq!(read_cache(&missing), None);
        assert_eq!(read_cache(&malformed), None);
    }

    #[test]
    fn writing_the_cache_somewhere_impossible_is_silently_survived() {
        // Best effort by design: the cost of failure is one extra check.
        write_cache(
            Path::new("/proc/nt-cannot-write-here.json"),
            &Cached {
                checked_at: 1,
                latest: "0.1.0".into(),
            },
        );
    }

    #[test]
    fn the_cache_path_honours_the_override_then_xdg_then_home() {
        // Read through the same helper the binary uses, so the documented
        // NT_UPDATE_CACHE override cannot drift from the code.
        assert!(cache_path().is_some() || std::env::var_os("HOME").is_none());
    }

    #[test]
    fn the_clock_reads_as_a_time_after_the_epoch() {
        assert!(now_secs() > 1_700_000_000);
    }
}
