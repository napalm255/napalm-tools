//! Command-line surface.
//!
//! Every flag is declared on exactly the commands where it does something,
//! so `--help` for a command shows only what applies to it and a
//! contradiction such as `nt version --quiet` is refused rather than accepted
//! and ignored. Bundle and prompt names are validated by clap against the
//! catalog, which also gives completions the values.

use clap::{Arg, ArgAction, ArgMatches, Command, builder::PossibleValuesParser};

use crate::bundles::BUNDLES;
use crate::config::{CliOverrides, PROMPTS};

/// Build the full command tree.
pub fn command() -> Command {
    let apply = with_selection_flags(with_common_flags(
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
                    .help("Exit 2 if any package has no provider here"),
            )
            .arg(
                Arg::new("no-dotfiles")
                    .long("no-dotfiles")
                    .action(ArgAction::SetTrue)
                    .help("Skip the dotfiles step"),
            )
            .arg(
                Arg::new("prompt")
                    .long("prompt")
                    .value_name("NAME")
                    .value_parser(PossibleValuesParser::new(PROMPTS))
                    .help("Shell prompt to install and activate [default: from config, else starship]"),
            )
            .arg(quiet_flag()),
    ));

    let status = with_detail_flag(with_selection_flags(with_common_flags(
        Command::new("status").about("Report desired versus installed state; changes nothing"),
    )));

    let bundles = with_detail_flag(with_selection_flags(with_common_flags(
        Command::new("bundles").about("List the catalog and each bundle's state here"),
    )));

    // `self` rather than a top-level `update`, which would read as "update
    // my packages" beside `apply --upgrade`. Neither takes --config: the
    // [update] check setting governs the automatic notice, and an explicit
    // request must not be silenced by a file.
    let self_cmd = Command::new("self")
        .about("Inspect and update nt itself")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("check")
                .about("Report whether a newer nt has been released; changes nothing")
                .arg(output_flag())
                .arg(verbose_flag()),
        )
        .subcommand(
            Command::new("update")
                .about("Download the latest release and replace this binary")
                .long_about(
                    "Download the latest release and replace this binary.\n\n\
                     Refuses when another tool owns the binary - Homebrew, cargo, \n\
                     mise or the system - and names what to run instead.",
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("Say what would be installed without downloading it"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Install even if this binary is already current"),
                )
                .arg(
                    Arg::new("version")
                        .long("version")
                        .value_name("X.Y.Z")
                        .help("Install this release instead of the latest; implies --force"),
                )
                .arg(output_flag())
                .arg(verbose_flag())
                .arg(quiet_flag()),
        );

    Command::new("nt")
        .about("Fast, private, idempotent user-space system configuration")
        .version(clap::crate_version!())
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(apply)
        .subcommand(status)
        .subcommand(bundles)
        .subcommand(
            Command::new("config")
                .about("Inspect configuration")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(with_selection_flags(with_common_flags(
                    Command::new("show").about("Print the resolved configuration"),
                )))
                .subcommand(
                    Command::new("path")
                        .about("Print the configuration file path")
                        .arg(config_flag()),
                ),
        )
        .subcommand(
            Command::new("shell-init")
                .about("Print the shell code that activates the configured prompt")
                .long_about(
                    "Print the shell code that activates the configured prompt.\n\n\
                     Add to your shell's start-up file:\n  eval \"$(nt shell-init bash)\"",
                )
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(PossibleValuesParser::new(SHELLS))
                        .help("Shell to generate for"),
                )
                .arg(config_flag()),
        )
        .subcommand(self_cmd)
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

/// Shells `shell-init` can target.
pub const SHELLS: &[&str] = &["bash", "zsh", "fish"];

/// Extract bundle selection and run options from parsed arguments.
pub fn overrides_from(matches: &ArgMatches) -> CliOverrides {
    let many = |name: &str| -> Vec<String> {
        matches
            .try_get_many::<String>(name)
            .ok()
            .flatten()
            .map(|v| v.cloned().collect())
            .unwrap_or_default()
    };
    let flag = |name: &str| -> Option<bool> {
        matches
            .try_get_one::<bool>(name)
            .ok()
            .flatten()
            .copied()
            .filter(|v| *v)
    };

    CliOverrides {
        skip: many("skip"),
        only: many("only"),
        upgrade: flag("upgrade"),
        strict: flag("strict"),
        dotfiles_enabled: flag("no-dotfiles").map(|_| false),
        prompt: matches
            .try_get_one::<String>("prompt")
            .ok()
            .flatten()
            .cloned(),
    }
}

/// Read the configuration file. For every command that resolves settings.
fn config_flag() -> Arg {
    Arg::new("config")
        .long("config")
        .value_name("PATH")
        .help("Use this configuration file instead of the default")
}

/// Choose how output is rendered.
fn output_flag() -> Arg {
    Arg::new("output")
        .long("output")
        .value_name("FORMAT")
        .value_parser(PossibleValuesParser::new(["pretty", "plain", "json"]))
        .help("Output format [default: pretty on a terminal, plain otherwise]")
}

/// Show raw subprocess output and raise the logging level.
fn verbose_flag() -> Arg {
    Arg::new("verbose")
        .short('v')
        .long("verbose")
        .action(ArgAction::Count)
        .help("Show raw command output; repeat to add debug logging")
}

/// Silence on success. Only meaningful where a command reports on work done.
fn quiet_flag() -> Arg {
    Arg::new("quiet")
        .short('q')
        .long("quiet")
        .action(ArgAction::SetTrue)
        .help("Say nothing unless something fails")
}

/// Attach the flags shared by every command that resolves configuration.
fn with_common_flags(cmd: Command) -> Command {
    cmd.arg(config_flag())
        .arg(output_flag())
        .arg(verbose_flag())
}

/// Attach `--skip` and `--only`, validated against the catalog.
fn with_selection_flags(cmd: Command) -> Command {
    let names: Vec<&'static str> = BUNDLES.iter().map(|b| b.name).collect();
    cmd.arg(
        Arg::new("skip")
            .long("skip")
            .value_name("BUNDLE")
            .action(ArgAction::Append)
            .value_parser(PossibleValuesParser::new(names.clone()))
            .help("Leave this bundle out for this run (repeatable)"),
    )
    .arg(
        Arg::new("only")
            .long("only")
            .value_name("BUNDLE")
            .action(ArgAction::Append)
            .value_parser(PossibleValuesParser::new(names))
            .help("Consider only this bundle for this run (repeatable)"),
    )
}

/// Attach `--detail`.
fn with_detail_flag(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("detail")
            .long("detail")
            .action(ArgAction::SetTrue)
            .help("Show every package, not just each bundle's summary"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse an argv, panicking on error, for terse tests.
    fn parse(args: &[&str]) -> ArgMatches {
        command().try_get_matches_from(args).unwrap()
    }

    fn accepted(argv: &[&str]) -> bool {
        command().try_get_matches_from(argv).is_ok()
    }

    #[test]
    fn skip_and_only_collect_bundle_names() {
        let m = parse(&[
            "nt", "apply", "--skip", "android", "--skip", "fonts", "--only", "core",
        ]);
        let o = overrides_from(m.subcommand_matches("apply").unwrap());

        assert_eq!(o.skip, vec!["android", "fonts"]);
        assert_eq!(o.only, vec!["core"]);
    }

    #[test]
    fn an_unknown_bundle_is_rejected_by_the_parser() {
        assert!(!accepted(&["nt", "apply", "--skip", "nope"]));
        assert!(!accepted(&["nt", "status", "--only", "nope"]));
    }

    #[test]
    fn every_catalog_bundle_is_a_valid_value() {
        for b in BUNDLES {
            assert!(accepted(&["nt", "bundles", "--skip", b.name]), "{}", b.name);
        }
    }

    #[test]
    fn nothing_given_means_nothing_overridden() {
        let m = parse(&["nt", "apply"]);
        let o = overrides_from(m.subcommand_matches("apply").unwrap());

        assert!(o.skip.is_empty() && o.only.is_empty());
        assert_eq!(o.upgrade, None);
        assert_eq!(o.strict, None);
        assert_eq!(o.dotfiles_enabled, None);
        assert_eq!(o.prompt, None);
    }

    #[test]
    fn run_options_are_set_when_given() {
        let m = parse(&[
            "nt",
            "apply",
            "--upgrade",
            "--strict",
            "--no-dotfiles",
            "--prompt",
            "oh-my-posh",
        ]);
        let o = overrides_from(m.subcommand_matches("apply").unwrap());

        assert_eq!(o.upgrade, Some(true));
        assert_eq!(o.strict, Some(true));
        assert_eq!(o.dotfiles_enabled, Some(false));
        assert_eq!(o.prompt.as_deref(), Some("oh-my-posh"));
    }

    #[test]
    fn an_unknown_prompt_is_rejected_by_the_parser() {
        assert!(!accepted(&["nt", "apply", "--prompt", "p10k"]));
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
            "self",
            "shell-init",
            "version",
            "completions",
        ] {
            assert!(names.contains(&expected), "missing subcommand {expected}");
        }
    }

    #[test]
    fn completions_and_shell_init_require_a_known_shell() {
        assert!(!accepted(&["nt", "completions"]));
        assert!(accepted(&["nt", "completions", "bash"]));
        assert!(!accepted(&["nt", "completions", "tcsh"]));
        assert!(!accepted(&["nt", "shell-init"]));
        assert!(accepted(&["nt", "shell-init", "bash"]));
        assert!(!accepted(&["nt", "shell-init", "tcsh"]));
    }

    // --- flags belong only where they mean something ------------------------

    #[test]
    fn selection_flags_reach_every_command_that_reports_resolved_state() {
        for argv in [
            vec!["nt", "apply", "--only", "core"],
            vec!["nt", "status", "--only", "core"],
            vec!["nt", "bundles", "--only", "core"],
            vec!["nt", "config", "show", "--only", "core"],
        ] {
            assert!(accepted(&argv), "{argv:?} should be accepted");
        }
    }

    #[test]
    fn detail_belongs_to_status_and_bundles_only() {
        assert!(accepted(&["nt", "status", "--detail"]));
        assert!(accepted(&["nt", "bundles", "--detail"]));
        assert!(!accepted(&["nt", "apply", "--detail"]));
        assert!(!accepted(&["nt", "config", "show", "--detail"]));
    }

    #[test]
    fn run_options_belong_to_apply_only() {
        for flag in ["--dry-run", "--upgrade", "--strict", "--no-dotfiles", "-q"] {
            assert!(accepted(&["nt", "apply", flag]), "{flag}");
            assert!(!accepted(&["nt", "status", flag]), "{flag} on status");
            assert!(!accepted(&["nt", "bundles", flag]), "{flag} on bundles");
        }
        assert!(accepted(&["nt", "apply", "--prompt", "starship"]));
        assert!(!accepted(&["nt", "bundles", "--prompt", "starship"]));
    }

    #[test]
    fn version_and_completions_take_no_flags_at_all() {
        for argv in [
            vec!["nt", "version", "-q"],
            vec!["nt", "version", "-v"],
            vec!["nt", "version", "--output", "json"],
            vec!["nt", "version", "--config", "/tmp/x.toml"],
            vec!["nt", "completions", "bash", "-q"],
            vec!["nt", "completions", "bash", "--output", "json"],
        ] {
            assert!(!accepted(&argv), "{argv:?} should be rejected");
        }
    }

    #[test]
    fn config_reaches_every_command_that_reads_configuration() {
        for argv in [
            vec!["nt", "apply", "--config", "/tmp/x.toml"],
            vec!["nt", "status", "--config", "/tmp/x.toml"],
            vec!["nt", "bundles", "--config", "/tmp/x.toml"],
            vec!["nt", "config", "path", "--config", "/tmp/x.toml"],
            vec!["nt", "config", "show", "--config", "/tmp/x.toml"],
            vec!["nt", "shell-init", "bash", "--config", "/tmp/x.toml"],
        ] {
            assert!(accepted(&argv), "{argv:?} should be accepted");
        }
    }

    #[test]
    fn output_and_verbosity_reach_the_commands_that_produce_reports() {
        for argv in [
            vec!["nt", "apply", "--output", "json", "-vv"],
            vec!["nt", "status", "--output", "json", "-v"],
            vec!["nt", "bundles", "--output", "json"],
            vec!["nt", "config", "show", "--output", "json"],
        ] {
            assert!(accepted(&argv), "{argv:?} should be accepted");
        }
        assert!(!accepted(&["nt", "config", "path", "--output", "json"]));
        assert!(!accepted(&["nt", "shell-init", "bash", "--output", "json"]));
        assert!(!accepted(&["nt", "bundles", "--output", "yaml"]));
    }

    #[test]
    fn help_does_not_list_a_flag_per_bundle() {
        let mut cmd = command();
        let apply = cmd
            .get_subcommands_mut()
            .find(|s| s.get_name() == "apply")
            .unwrap();
        let help = apply.render_long_help().to_string();

        assert!(!help.contains("--no-core"), "{help}");
        assert!(help.contains("--skip"), "{help}");
        assert!(help.contains("raw command output"), "{help}");
    }

    #[test]
    fn self_requires_one_of_its_two_subcommands() {
        assert!(!accepted(&["nt", "self"]));
        assert!(accepted(&["nt", "self", "check"]));
        assert!(accepted(&["nt", "self", "update"]));
        assert!(!accepted(&["nt", "self", "upgrade"]));
    }

    #[test]
    fn only_self_update_takes_the_flags_that_change_what_is_installed() {
        for flag in ["--dry-run", "--force"] {
            assert!(accepted(&["nt", "self", "update", flag]), "{flag}");
            assert!(!accepted(&["nt", "self", "check", flag]), "{flag}");
        }
        assert!(accepted(&["nt", "self", "update", "--version", "0.2.0"]));
        assert!(!accepted(&["nt", "self", "check", "--version", "0.2.0"]));
        assert!(accepted(&["nt", "self", "update", "-q"]));
        assert!(!accepted(&["nt", "self", "check", "-q"]));
    }

    #[test]
    fn self_reports_through_the_usual_output_flags() {
        assert!(accepted(&["nt", "self", "check", "--output", "json"]));
        assert!(accepted(&[
            "nt", "self", "update", "--output", "json", "-v"
        ]));
        assert!(!accepted(&["nt", "self", "check", "--output", "yaml"]));
    }

    #[test]
    fn self_reads_no_configuration_so_it_takes_no_config_flag() {
        // The [update] check setting governs the automatic notice only; an
        // explicit `nt self check` must not be silenced by a file.
        assert!(!accepted(&["nt", "self", "check", "--config", "/x.toml"]));
        assert!(!accepted(&["nt", "self", "update", "--config", "/x.toml"]));
    }

    #[test]
    fn self_takes_no_bundle_selection_or_detail() {
        assert!(!accepted(&["nt", "self", "check", "--detail"]));
        assert!(!accepted(&["nt", "self", "update", "--detail"]));
        assert!(!accepted(&["nt", "self", "check", "--only", "core"]));
        assert!(!accepted(&["nt", "self", "update", "--skip", "core"]));
    }
}
