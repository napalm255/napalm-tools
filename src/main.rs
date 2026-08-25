//! `nt` - napalm-tools.

use anyhow::{Context, Result};
use clap::ArgMatches;
use std::path::PathBuf;
use std::process::ExitCode;

use napalm_tools::{cli, config, dotfiles, execute, plan, platform, report};

/// Exit code used when `--strict` is set and some package has no provider.
const EXIT_UNMET: u8 = 2;

fn main() -> ExitCode {
    let matches = cli::command().get_matches();
    init_logging(&matches);

    match dispatch(&matches) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// Configure logging from `-v` / `-q`, letting `RUST_LOG` win if it is set.
fn init_logging(matches: &ArgMatches) {
    let default = if matches.get_flag("quiet") {
        "error"
    } else {
        match matches.get_count("verbose") {
            0 => "warn",
            1 => "info",
            _ => "debug",
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

fn dispatch(matches: &ArgMatches) -> Result<ExitCode> {
    let quiet = matches.get_flag("quiet");

    match matches.subcommand() {
        Some(("version", _)) => {
            // Deliberately bare, so it can be piped into other scripts.
            println!("{}", clap::crate_version!());
            Ok(ExitCode::SUCCESS)
        }

        Some(("completions", sub)) => {
            let shell = *sub
                .get_one::<clap_complete::Shell>("shell")
                .expect("shell is required");
            clap_complete::generate(shell, &mut cli::command(), "nt", &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }

        Some(("config", sub)) => match sub.subcommand() {
            Some(("path", _)) => {
                println!("{}", config_path(matches).display());
                Ok(ExitCode::SUCCESS)
            }
            Some(("show", _)) => {
                let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
                print!("{}", report::render_resolved(&resolved, &platform));
                Ok(ExitCode::SUCCESS)
            }
            _ => unreachable!("clap requires a config subcommand"),
        },

        Some(("bundles", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            print!("{}", report::render_bundles(&resolved, &platform));
            Ok(ExitCode::SUCCESS)
        }

        Some(("status", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            let snapshot = execute::snapshot(&platform)?;
            let built = plan::build(&resolved, &platform, &snapshot);

            for (manager, count) in execute::explicit_packages(&platform)? {
                println!("{manager:<10} {count} explicitly installed");
            }
            println!();
            // Not a dry run - status simply never acts, so the banner would
            // be misleading here.
            print!("{}", report::render_plan(&built, false));
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

            if dry_run {
                print!("{}", report::render_plan(&built, true));
            } else {
                if !built.is_empty() {
                    execute::run(&built, quiet)?;
                }
                if built.is_empty() {
                    println!("Nothing to do.");
                }
                // Notes are shown whether or not anything ran, so an
                // unprovisionable package is never silently dropped.
                print!("{}", report::render_notes(&built));
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
