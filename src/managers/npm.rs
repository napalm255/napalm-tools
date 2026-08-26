//! npm global installs.

use anyhow::{Context, Result};
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The npm manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Npm;

/// Parse `npm ls -g --depth=0 --json` output into package names.
///
/// Scoped packages keep their `@scope/` prefix, which is how they are named on
/// the command line.
pub fn parse_global_json(output: &str) -> Result<HashSet<String>> {
    if output.trim().is_empty() {
        return Ok(HashSet::new());
    }
    let root: serde_json::Value =
        serde_json::from_str(output).context("failed to parse npm JSON output")?;
    Ok(root
        .get("dependencies")
        .and_then(|d| d.as_object())
        .map(|deps| deps.keys().cloned().collect())
        .unwrap_or_default())
}

impl Manager for Npm {
    fn id(&self) -> ManagerId {
        ManagerId::Npm
    }

    fn binary(&self) -> &'static str {
        "npm"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        // `npm ls` exits non-zero when the global tree has any problem, even a
        // benign one, so its status is deliberately ignored in favour of
        // whatever JSON it produced.
        let cmd = Cmd::new("npm", ["ls", "-g", "--depth=0", "--json"]);
        let out = cmd
            .to_command()
            .output()
            .context("failed to run `npm ls -g --depth=0 --json`")?;
        if !out.status.success() {
            tracing::debug!(
                status = %out.status,
                "npm ls exited non-zero; using whatever JSON it produced"
            );
        }
        parse_global_json(&String::from_utf8_lossy(&out.stdout))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("npm", &["install", "-g"], packages)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("npm", &["update", "-g"], packages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_dependencies() {
        let json =
            r#"{"dependencies":{"openclaw":{"version":"2026.4.23"},"npm":{"version":"11.17.0"}}}"#;

        let set = parse_global_json(json).unwrap();

        assert!(set.contains("openclaw"));
        assert!(set.contains("npm"));
    }

    #[test]
    fn keeps_the_scope_on_scoped_packages() {
        let json = r#"{"dependencies":{"@anthropic-ai/claude-code":{"version":"1.0.0"}}}"#;

        let set = parse_global_json(json).unwrap();

        assert!(
            set.contains("@anthropic-ai/claude-code"),
            "scoped names must survive intact: {set:?}"
        );
    }

    #[test]
    fn output_without_dependencies_is_an_empty_set() {
        let set = parse_global_json(r#"{"name":"lib"}"#).unwrap();

        assert!(set.is_empty());
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        // npm produces nothing at all when the global prefix is missing.
        let set = parse_global_json("").unwrap();

        assert!(set.is_empty());
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_global_json("{not json").is_err());
    }

    #[test]
    fn global_flag_is_always_present() {
        let cmd = Npm.install_cmd(&["openclaw".into()]);

        assert_eq!(cmd.to_shell(), "npm install -g openclaw");
    }
}
