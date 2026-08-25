//! Command-line surface.
//!
//! Bundle flags are emitted by iterating the catalog rather than being written
//! out by hand, so a new bundle cannot ship without its flags. That is the
//! whole parity guarantee: there is only one list.

use clap::{Arg, ArgAction, ArgMatches, Command};
use std::collections::BTreeMap;

use crate::bundles::BUNDLES;
use crate::config::CliOverrides;

/// The `--no-` prefix used for the negative form of a bundle flag.
const NEGATIVE_PREFIX: &str = "no-";

/// Build the full command tree.
pub fn command() -> Command {
    let apply = with_bundle_flags(
        Command::new("apply")
            .about("Converge this machine on the resolved configuration")
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue)
                    .help("Show what would happen without changing anything"),
            )
            .arg(
                Arg::new("upgrade")
                    .long("upgrade")
                    .action(ArgAction::SetTrue)
                    .help("Also upgrade packages that are already installed"),
            )
            .arg(
                Arg::new("strict")
                    .long("strict")
                    .action(ArgAction::SetTrue)
                    .help("Exit non-zero if any package has no provider here"),
            )
            .arg(
                Arg::new("no-dotfiles")
                    .long("no-dotfiles")
                    .action(ArgAction::SetTrue)
                    .help("Skip the dotfiles step"),
            ),
    );

    let status = with_bundle_flags(
        Command::new("status").about("Report desired versus installed state; changes nothing"),
    );

    Command::new("nt")
        .about("Fast, private, idempotent user-space system configuration")
        .version(clap::crate_version!())
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count)
                .global(true)
                .help("Increase logging detail; repeat for more"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Suppress progress output"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .value_name("PATH")
                .global(true)
                .help("Use this configuration file instead of the default"),
        )
        .subcommand(apply)
        .subcommand(status)
        .subcommand(with_bundle_flags(
            Command::new("bundles").about("List bundles and their effective state"),
        ))
        .subcommand(
            Command::new("config")
                .about("Inspect configuration")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(with_bundle_flags(
                    Command::new("show").about("Print the resolved configuration"),
                ))
                .subcommand(Command::new("path").about("Print the configuration file path")),
        )
        .subcommand(Command::new("version").about("Print the version alone, for scripts"))
        .subcommand(
            Command::new("completions")
                .about("Generate a shell completion script")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(clap::builder::EnumValueParser::<clap_complete::Shell>::new())
                        .help("Shell to generate for"),
                ),
        )
}

/// Extract bundle toggles and run options from parsed arguments.
pub fn overrides_from(matches: &ArgMatches) -> CliOverrides {
    let mut bundles = BTreeMap::new();
    for b in BUNDLES {
        let negative = format!("{NEGATIVE_PREFIX}{}", b.name);
        // The negative form is checked first so `--x --no-x` resolves to off.
        if matches.try_get_one::<bool>(&negative).ok().flatten() == Some(&true) {
            bundles.insert(b.name.to_string(), false);
        } else if matches.try_get_one::<bool>(b.name).ok().flatten() == Some(&true) {
            bundles.insert(b.name.to_string(), true);
        }
    }

    let flag = |name: &str| -> Option<bool> {
        matches
            .try_get_one::<bool>(name)
            .ok()
            .flatten()
            .copied()
            .filter(|v| *v)
    };

    CliOverrides {
        bundles,
        upgrade: flag("upgrade"),
        strict: flag("strict"),
        dotfiles_enabled: flag("no-dotfiles").map(|_| false),
    }
}

/// Attach the generated `--<bundle>` / `--no-<bundle>` flags to a subcommand.
fn with_bundle_flags(mut cmd: Command) -> Command {
    for b in BUNDLES {
        let negative = format!("{NEGATIVE_PREFIX}{}", b.name);
        cmd = cmd
            .arg(
                Arg::new(b.name)
                    .long(b.name)
                    .action(ArgAction::SetTrue)
                    .overrides_with(negative.clone())
                    .help(format!("Enable: {}", b.description)),
            )
            .arg(
                Arg::new(negative.clone())
                    .long(negative)
                    .action(ArgAction::SetTrue)
                    .overrides_with(b.name)
                    .help(format!("Disable: {}", b.description)),
            );
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse an argv, panicking on error, for terse tests.
    fn parse(args: &[&str]) -> ArgMatches {
        command().try_get_matches_from(args).unwrap()
    }

    #[test]
    fn every_bundle_gets_both_flags() {
        // The parity guarantee: adding a bundle adds its flags automatically.
        let cmd = command();
        let apply = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "apply")
            .expect("apply subcommand");
        let longs: Vec<String> = apply
            .get_arguments()
            .filter_map(|a| a.get_long().map(str::to_string))
            .collect();

        for b in BUNDLES {
            assert!(longs.contains(&b.name.to_string()), "missing --{}", b.name);
            assert!(
                longs.contains(&format!("no-{}", b.name)),
                "missing --no-{}",
                b.name
            );
        }
    }

    #[test]
    fn the_negative_flag_disables_a_bundle() {
        let m = parse(&["nt", "apply", "--no-go-runtime"]);
        let sub = m.subcommand_matches("apply").unwrap();

        let o = overrides_from(sub);

        assert_eq!(o.bundles.get("go-runtime"), Some(&false));
    }

    #[test]
    fn the_positive_flag_enables_a_bundle() {
        let m = parse(&["nt", "apply", "--aws"]);
        let sub = m.subcommand_matches("apply").unwrap();

        let o = overrides_from(sub);

        assert_eq!(o.bundles.get("aws"), Some(&true));
    }

    #[test]
    fn an_unmentioned_bundle_is_left_to_the_configuration() {
        let m = parse(&["nt", "apply"]);
        let sub = m.subcommand_matches("apply").unwrap();

        let o = overrides_from(sub);

        assert!(o.bundles.is_empty(), "got {:?}", o.bundles);
    }

    #[test]
    fn the_last_of_a_conflicting_pair_wins() {
        let m = parse(&["nt", "apply", "--no-aws", "--aws"]);
        let sub = m.subcommand_matches("apply").unwrap();

        let o = overrides_from(sub);

        assert_eq!(o.bundles.get("aws"), Some(&true), "later flag should win");
    }

    #[test]
    fn run_options_are_only_set_when_given() {
        let bare = parse(&["nt", "apply"]);
        let o = overrides_from(bare.subcommand_matches("apply").unwrap());
        assert_eq!(o.upgrade, None);
        assert_eq!(o.strict, None);
        assert_eq!(o.dotfiles_enabled, None);

        let full = parse(&["nt", "apply", "--upgrade", "--strict", "--no-dotfiles"]);
        let o = overrides_from(full.subcommand_matches("apply").unwrap());
        assert_eq!(o.upgrade, Some(true));
        assert_eq!(o.strict, Some(true));
        assert_eq!(o.dotfiles_enabled, Some(false));
    }

    #[test]
    fn apply_accepts_dry_run() {
        let m = parse(&["nt", "apply", "--dry-run"]);
        let sub = m.subcommand_matches("apply").unwrap();

        assert!(sub.get_flag("dry-run"));
    }

    #[test]
    fn the_expected_subcommands_exist() {
        let cmd = command();
        let names: Vec<&str> = cmd.get_subcommands().map(|s| s.get_name()).collect();

        for expected in [
            "apply",
            "status",
            "bundles",
            "config",
            "version",
            "completions",
        ] {
            assert!(names.contains(&expected), "missing subcommand {expected}");
        }
    }

    #[test]
    fn config_has_show_and_path_subcommands() {
        // The bare `nt config` invocation is reserved for a future interface,
        // so the foundation exposes explicit subcommands under it.
        let cmd = command();
        let config = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "config")
            .unwrap();
        let names: Vec<&str> = config.get_subcommands().map(|s| s.get_name()).collect();

        assert!(names.contains(&"show"));
        assert!(names.contains(&"path"));
    }

    #[test]
    fn completions_requires_a_shell() {
        assert!(
            command()
                .try_get_matches_from(["nt", "completions"])
                .is_err()
        );
        assert!(
            command()
                .try_get_matches_from(["nt", "completions", "bash"])
                .is_ok()
        );
    }

    #[test]
    fn an_unknown_shell_is_rejected() {
        assert!(
            command()
                .try_get_matches_from(["nt", "completions", "tcsh"])
                .is_err()
        );
    }

    #[test]
    fn status_also_takes_bundle_flags() {
        // status reports against the same resolved configuration as apply, so
        // it has to accept the same overrides.
        let m = parse(&["nt", "status", "--no-desktop"]);
        let sub = m.subcommand_matches("status").unwrap();

        assert_eq!(overrides_from(sub).bundles.get("desktop"), Some(&false));
    }

    #[test]
    fn every_command_that_reports_resolved_state_accepts_bundle_flags() {
        // Otherwise `nt bundles --aws` fails while `nt apply --aws` works,
        // which breaks the parity the flags exist to provide.
        for argv in [
            vec!["nt", "apply", "--aws"],
            vec!["nt", "status", "--aws"],
            vec!["nt", "bundles", "--aws"],
            vec!["nt", "config", "show", "--aws"],
        ] {
            assert!(
                command().try_get_matches_from(&argv).is_ok(),
                "{argv:?} should be accepted"
            );
        }
    }
}
