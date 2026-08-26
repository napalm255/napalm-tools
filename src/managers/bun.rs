//! bun global installs.

use anyhow::Result;
use std::collections::HashSet;

use super::{Cmd, Manager, ManagerId};
use crate::platform::Platform;

/// The bun manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bun;

/// Parse `bun pm ls -g` output into package names.
pub fn parse_list(output: &str) -> HashSet<String> {
    output
        .lines()
        // Strip the box-drawing characters bun uses to render the tree.
        .map(|l| {
            l.trim_matches(|c: char| {
                c.is_whitespace() || "\u{251c}\u{2514}\u{2500}\u{2502}".contains(c)
            })
        })
        .filter_map(|l| {
            // Entries are `name@version`; the header line carries no version.
            let (name, _version) = l.rsplit_once('@')?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

impl Manager for Bun {
    fn id(&self) -> ManagerId {
        ManagerId::Bun
    }

    fn binary(&self) -> &'static str {
        "bun"
    }

    fn platform_ok(&self, _platform: &Platform) -> bool {
        true
    }

    fn installed(&self) -> Result<HashSet<String>> {
        Ok(parse_list(&Cmd::new("bun", ["pm", "ls", "-g"]).output()?))
    }

    fn install_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("bun", &["add", "-g"], packages)
    }

    fn upgrade_cmd(&self, packages: &[String]) -> Cmd {
        Cmd::with_packages("bun", &["update", "-g"], packages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_tree_output_into_names() {
        // `bun pm ls -g` prints a header line and then a tree.
        let set = parse_list(
            "/home/napalm/.bun/install/global node_modules\n├── cowsay@1.6.0\n└── typescript@5.4.0\n",
        );

        assert!(set.contains("cowsay"), "got {set:?}");
        assert!(set.contains("typescript"), "got {set:?}");
    }

    #[test]
    fn keeps_the_scope_on_scoped_packages() {
        let set = parse_list("dir node_modules\n└── @scope/pkg@1.0.0\n");

        assert!(set.contains("@scope/pkg"), "got {set:?}");
    }

    #[test]
    fn empty_output_is_an_empty_set() {
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn add_is_the_install_verb() {
        let cmd = Bun.install_cmd(&["cowsay".into()]);

        assert_eq!(cmd.to_shell(), "bun add -g cowsay");
    }
}
