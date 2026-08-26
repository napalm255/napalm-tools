//! Playwright's browser store, used as the one user-space Chromium.
//!
//! A system Chromium is not an option everywhere `nt` runs: `dnf` is refused
//! on atomic hosts, and the Flatpak is sandboxed and desktop-only.
//! `playwright install chromium` is user-space on every target and
//! idempotent, so the browser is treated as a package with its own manager
//! and the install is planned only when no complete revision is present.

use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The Playwright browser manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Playwright;

/// The one package this manager supplies.
pub const CHROMIUM: &str = "chromium";

/// Where Playwright keeps browsers: `$PLAYWRIGHT_BROWSERS_PATH`, else
/// `~/.cache/ms-playwright`.
pub fn browsers_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".cache/ms-playwright"))
}

/// The Chromium executable under `browsers_dir`, newest revision first.
/// Pure apart from reading that directory.
pub fn chromium_path(browsers_dir: &Path) -> Option<PathBuf> {
    let mut revisions: Vec<(u64, PathBuf)> = std::fs::read_dir(browsers_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // `chromium-1187`, not `chromium_headless_shell-1187`.
            let rev = name.strip_prefix("chromium-")?.parse::<u64>().ok()?;
            let exe = e.path().join("chrome-linux").join("chrome");
            exe.is_file().then_some((rev, exe))
        })
        .collect();
    revisions.sort_by_key(|(rev, _)| std::cmp::Reverse(*rev));
    revisions.into_iter().next().map(|(_, p)| p)
}

impl Manager for Playwright {
    fn id(&self) -> ManagerId {
        ManagerId::Playwright
    }

    fn binary(&self) -> &'static str {
        // The npm-installed CLI from the `web` bundle; `npx` would fetch a
        // second copy of the whole package on a fresh machine.
        "playwright"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        let mut set = HashSet::new();
        if browsers_dir().and_then(|d| chromium_path(&d)).is_some() {
            set.insert(CHROMIUM.to_string());
        }
        Ok(set)
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("playwright", &["install"], packages).in_home()
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        // The same command: it fetches the revision the current Playwright
        // pins, which is what an upgrade means here.
        self.install_cmd(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(dir: &Path, name: &str, with_exe: bool) {
        let d = dir.join(name).join("chrome-linux");
        std::fs::create_dir_all(&d).unwrap();
        if with_exe {
            std::fs::write(d.join("chrome"), "").unwrap();
        }
    }

    #[test]
    fn the_newest_complete_revision_is_chosen() {
        let dir = tempfile::tempdir().unwrap();
        make(dir.path(), "chromium-1100", true);
        make(dir.path(), "chromium-1187", true);
        make(dir.path(), "chromium-1200", false); // half-downloaded
        make(dir.path(), "chromium_headless_shell-1187", true);

        let p = chromium_path(dir.path()).unwrap();

        assert!(p.ends_with("chromium-1187/chrome-linux/chrome"), "{p:?}");
    }

    #[test]
    fn no_browser_means_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(chromium_path(dir.path()).is_none());
        assert!(chromium_path(Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn install_is_idempotent_and_non_interactive() {
        let cmd = Playwright.install_cmd(&[CHROMIUM.into()]);

        assert_eq!(cmd.to_shell(), "playwright install chromium");
        assert!(!cmd.privileged);
        assert_eq!(
            Playwright.upgrade_cmd(&[CHROMIUM.into()]).to_shell(),
            cmd.to_shell()
        );
    }

    #[test]
    fn playwright_is_usable_everywhere() {
        use crate::platform::test_platforms::*;
        for p in [ATOMIC, PLAIN, SERVER, UNDER_WSL, CONTAINER] {
            assert!(Playwright.platform_ok(&p), "{p:?}");
        }
    }
}
