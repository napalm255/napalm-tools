//! mise, for language toolchains.
//!
//! Provider ids are `tool@version` - `java@corretto-21`, `node@lts`,
//! `python@3.13` - exactly as `mise use` takes them. Installs go to the
//! user's global configuration so the toolchain is available in every shell,
//! and the installed check reads that same configuration back: a tool is
//! present only if the global config asks for the same version spec and mise
//! reports it installed. That is what makes `java@corretto-21` distinct from
//! a Temurin the user happened to have.

use anyhow::{Context, Result};
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The mise manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mise;

/// Parse `mise ls --global --json` into the set of `tool@requested` specs
/// that are installed.
pub fn parse_global_json(output: &str) -> Result<HashSet<String>> {
    if output.trim().is_empty() {
        return Ok(HashSet::new());
    }
    let root: serde_json::Value =
        serde_json::from_str(output).context("failed to parse mise JSON output")?;
    let Some(tools) = root.as_object() else {
        return Ok(HashSet::new());
    };
    let mut set = HashSet::new();
    for (tool, versions) in tools {
        for v in versions.as_array().into_iter().flatten() {
            let installed = v
                .get("installed")
                .and_then(|i| i.as_bool())
                .unwrap_or(false);
            let requested = v.get("requested_version").and_then(|r| r.as_str());
            if let (true, Some(requested)) = (installed, requested) {
                set.insert(format!("{tool}@{requested}"));
            }
        }
    }
    Ok(set)
}

impl Manager for Mise {
    fn id(&self) -> ManagerId {
        ManagerId::Mise
    }

    fn binary(&self) -> &'static str {
        "mise"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        parse_global_json(
            &Cmd::new("mise", ["ls", "--global", "--json"])
                .in_home()
                .output()?,
        )
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("mise", &["use", "--global", "--yes"], packages).in_home()
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        // `mise upgrade` takes bare tool names or specs; specs are accepted.
        Cmd::with_packages("mise", &["upgrade", "--yes"], packages).in_home()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape taken from `mise ls --global --json` on the development machine.
    const GLOBAL: &str = r#"{
      "go": [{"version": "1.26.5", "requested_version": "latest", "installed": true}],
      "java": [{"version": "corretto-21.0.4", "requested_version": "corretto-21", "installed": true},
               {"version": "temurin-17.0.19+10", "requested_version": null, "installed": true}],
      "node": [{"version": "22.0.0", "requested_version": "lts", "installed": false}]
    }"#;

    #[test]
    fn an_installed_requested_tool_is_present_as_tool_at_spec() {
        let set = parse_global_json(GLOBAL).unwrap();

        assert!(set.contains("go@latest"), "got {set:?}");
        assert!(set.contains("java@corretto-21"), "got {set:?}");
    }

    #[test]
    fn a_version_nobody_asked_for_does_not_count() {
        // Temurin is on disk but not in the global config; it must not
        // satisfy a request for Corretto or for anything else.
        let set = parse_global_json(GLOBAL).unwrap();

        assert!(!set.iter().any(|s| s.contains("temurin")), "got {set:?}");
    }

    #[test]
    fn a_requested_but_uninstalled_tool_is_absent() {
        let set = parse_global_json(GLOBAL).unwrap();

        assert!(!set.contains("node@lts"), "got {set:?}");
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_global_json("").unwrap().is_empty());
        assert!(parse_global_json("{}").unwrap().is_empty());
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_global_json("{nope").is_err());
    }

    #[test]
    fn installs_go_to_the_global_config_without_prompting() {
        let cmd = Mise.install_cmd(&["java@corretto-21".into(), "node@lts".into()]);

        assert_eq!(
            cmd.to_shell(),
            "mise use --global --yes java@corretto-21 node@lts"
        );
        assert!(!cmd.privileged);
    }

    #[test]
    fn mise_commands_run_from_home_not_the_current_directory() {
        // A project mise.toml in the cwd - untrusted, or pinning other
        // versions - must not change what the global listing says.
        let cmd = Mise.install_cmd(&["go@latest".into()]);

        assert_eq!(
            cmd.cwd.as_deref(),
            Some(std::path::Path::new(&std::env::var("HOME").unwrap()))
        );
    }

    #[test]
    fn upgrade_is_distinct_from_install() {
        assert_eq!(
            Mise.upgrade_cmd(&["go@latest".into()]).to_shell(),
            "mise upgrade --yes go@latest"
        );
    }

    #[test]
    fn mise_is_usable_everywhere() {
        use crate::platform::test_platforms::*;
        for p in [ATOMIC, PLAIN, SERVER, UNDER_WSL, CONTAINER] {
            assert!(Mise.platform_ok(&p), "{p:?}");
        }
    }
}
