//! `nt` - napalm-tools.

use anyhow::{Context, Result};
use clap::ArgMatches;
use std::path::PathBuf;
use std::process::ExitCode;

use napalm_tools::ui::{Format, Ui, json};
use napalm_tools::{cli, config, dotfiles, execute, plan, platform, report};

/// Exit code used when `--strict` is set and some package has no provider.
const EXIT_UNMET: u8 = 2;

fn main() -> ExitCode {
    let matches = cli::command().get_matches();
    init_logging(&matches);

    let ui = Ui::new(
        format_from(&matches),
        matches.get_count("verbose"),
        matches.get_flag("quiet"),
    );

    match dispatch(&matches, &ui) {
        Ok(code) => code,
        Err(err) => {
            ui.error(&format!("{err:#}"));
            ExitCode::FAILURE
        }
    }
}

/// The output format: what was asked for, or what suits the terminal.
fn format_from(matches: &ArgMatches) -> Format {
    matches
        .get_one::<String>("output")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(Format::detect)
}

/// Configure logging from `-v` / `-q`, letting `RUST_LOG` win if it is set.
///
/// `-v` primarily switches subprocess output to raw passthrough; raising the
/// tracing level alongside it is the secondary effect.
fn init_logging(matches: &ArgMatches) {
    let default = if matches.get_flag("quiet") {
        "error"
    } else {
        match matches.get_count("verbose") {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
}

/// The configuration path in effect, honouring `--config`.
fn config_path(matches: &ArgMatches) -> PathBuf {
    matches
        .get_one::<String>("config")
        .map(PathBuf::from)
        .unwrap_or_else(config::default_path)
}

fn dispatch(matches: &ArgMatches, ui: &Ui) -> Result<ExitCode> {
    match matches.subcommand() {
        Some(("version", _)) => {
            // Deliberately bare, so it can be piped into other scripts.
            ui.data(&format!("{}\n", clap::crate_version!()));
            Ok(ExitCode::SUCCESS)
        }

        Some(("completions", sub)) => {
            let shell = *sub
                .get_one::<clap_complete::Shell>("shell")
                .expect("shell is required");
            // Through the Ui rather than straight to stdout, so `--quiet`
            // means quiet here too rather than having one exception.
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut cli::command(), "nt", &mut script);
            ui.data(&String::from_utf8_lossy(&script));
            Ok(ExitCode::SUCCESS)
        }

        Some(("config", sub)) => match sub.subcommand() {
            Some(("path", _)) => {
                ui.data(&format!("{}\n", config_path(matches).display()));
                Ok(ExitCode::SUCCESS)
            }
            Some(("show", show)) => {
                let (resolved, platform) = resolve(matches, &cli::overrides_from(show))?;
                ui.data(&match ui.format() {
                    Format::Json => json::to_string(&json::config_view(&resolved, &platform)),
                    _ => report::render_resolved(&resolved, &platform),
                });
                Ok(ExitCode::SUCCESS)
            }
            _ => unreachable!("clap requires a config subcommand"),
        },

        Some(("bundles", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            ui.data(&match ui.format() {
                Format::Json => json::to_string(&json::bundles_view(&resolved, &platform)),
                _ => report::render_bundles(&resolved, &platform),
            });
            Ok(ExitCode::SUCCESS)
        }

        Some(("status", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            let snapshot = execute::snapshot(&platform)?;
            let built = plan::build(&resolved, &platform, &snapshot);

            if ui.format() == Format::Json {
                ui.data(&json::to_string(&json::plan_view(&built, true)));
                return Ok(ExitCode::SUCCESS);
            }

            let mut out = String::new();
            for (manager, count) in execute::explicit_packages(&platform)? {
                out.push_str(&format!("{manager:<10} {count} explicitly installed\n"));
            }
            out.push('\n');
            // Not a dry run - status simply never acts, so the banner would
            // be misleading here.
            out.push_str(&report::render_plan(&built, false));
            ui.data(&out);
            Ok(ExitCode::SUCCESS)
        }

        Some(("apply", sub)) => {
            let dry_run = sub.get_flag("dry-run");
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            let snapshot = execute::snapshot(&platform)?;
            let mut built = plan::build(&resolved, &platform, &snapshot);

            let home = std::env::var("HOME").unwrap_or_default();
            let source_exists = dotfiles::source_dir(std::path::Path::new(&home)).exists();
            built.dotfiles = dotfiles::plan(&resolved.dotfiles, source_exists)?;

            let json_mode = ui.format() == Format::Json;

            if dry_run {
                ui.data(&if json_mode {
                    json::to_string(&json::plan_view(&built, true))
                } else {
                    report::render_plan(&built, true)
                });
            } else {
                let report = if built.is_empty() {
                    execute::RunReport::default()
                } else {
                    execute::run(&built, ui)?
                };

                if json_mode {
                    // One document carrying both what was planned and what
                    // happened, so a consumer needs only a single parse.
                    ui.data(&json::to_string(&serde_json::json!({
                        "plan": json::plan_view(&built, false),
                        "run": json::report_view(&report),
                    })));
                } else {
                    if built.is_empty() {
                        ui.data("Nothing to do.\n");
                    }
                    ui.summary(&report);
                    // Notes are shown whether or not anything ran, so an
                    // unprovisionable package is never silently dropped.
                    ui.data(&report::render_notes(&built));
                }
            }

            if resolved.strict && !built.unavailable.is_empty() {
                return Ok(ExitCode::from(EXIT_UNMET));
            }
            Ok(ExitCode::SUCCESS)
        }

        _ => unreachable!("clap requires a subcommand"),
    }
}

/// Load configuration, detect the platform, and resolve the two together.
fn resolve(
    matches: &ArgMatches,
    overrides: &config::CliOverrides,
) -> Result<(config::Resolved, platform::Platform)> {
    let path = config_path(matches);
    let file = config::load(&path)?;
    let hostname = config::hostmatch::hostname();
    let detected = platform::Platform::detect();
    let resolved = config::resolve(&file, &hostname, overrides)
        .with_context(|| format!("failed to resolve configuration from {}", path.display()))?;
    Ok((resolved, detected))
}
