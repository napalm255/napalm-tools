//! `nt` - napalm-tools.

#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use std::path::PathBuf;
use std::process::ExitCode;

use napalm_tools::ui::{Format, Ui, json};
use napalm_tools::{cli, config, dotfiles, execute, plan, platform, privilege, report, shell};

/// Exit code used when `--strict` is set and some package has no provider.
const EXIT_UNMET: u8 = 2;

fn main() -> ExitCode {
    let matches = cli::command().get_matches();
    // Flags are declared per subcommand rather than globally, so that
    // `nt version --config` is refused instead of accepted and ignored. That
    // means they arrive on the innermost matches.
    let flags = leaf(&matches);
    init_logging(flags);

    let ui = Ui::new(format_from(flags), verbosity(flags), quiet(flags));

    match dispatch(&matches, &ui) {
        Ok(code) => code,
        Err(err) => {
            ui.error(&format!("{err:#}"));
            ExitCode::FAILURE
        }
    }
}

/// How many times `-v` was given. Zero where the flag is not defined at all.
fn verbosity(matches: &ArgMatches) -> u8 {
    matches
        .try_get_one::<u8>("verbose")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0)
}

/// Whether `--quiet` was given. False where the flag is not defined.
fn quiet(matches: &ArgMatches) -> bool {
    matches.try_get_one::<bool>("quiet").ok().flatten() == Some(&true)
}

/// Whether `--detail` was given. False where the flag is not defined.
fn detail(matches: &ArgMatches) -> bool {
    matches.try_get_one::<bool>("detail").ok().flatten() == Some(&true)
}

/// The innermost subcommand's matches, where the shared flags land.
fn leaf(matches: &ArgMatches) -> &ArgMatches {
    let mut current = matches;
    while let Some((_, sub)) = current.subcommand() {
        current = sub;
    }
    current
}

/// The output format: what was asked for, or what suits the terminal.
fn format_from(matches: &ArgMatches) -> Format {
    matches
        .try_get_one::<String>("output")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(Format::detect)
}

/// Configure logging from `-v` / `-q`, letting `RUST_LOG` win if it is set.
fn init_logging(matches: &ArgMatches) {
    let default = if quiet(matches) {
        "error"
    } else {
        match verbosity(matches) {
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
    leaf(matches)
        .try_get_one::<String>("config")
        .ok()
        .flatten()
        .map(PathBuf::from)
        .unwrap_or_else(config::default_path)
}

/// Refuse to provision as root. Homebrew refuses too, and a root-owned
/// `~/.local` is a trap for every tool that comes after.
fn refuse_root() -> Result<()> {
    if is_root() {
        bail!(
            "nt apply must run as an ordinary user, not root; \
             create a user with sudo access and run it there"
        );
    }
    Ok(())
}

/// Whether the effective user is root, read from `/proc` so no crate is
/// needed for a single syscall's worth of information.
fn is_root() -> bool {
    std::env::var("NT_FAKE_UID")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/proc/self/status")
                .ok()?
                .lines()
                .find(|l| l.starts_with("Uid:"))?
                .split_whitespace()
                .nth(2)
                .map(str::to_string)
        })
        .is_some_and(|uid| uid == "0")
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
                .expect("clap enforces that shell is present");
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut cli::command(), "nt", &mut script);
            ui.data(&String::from_utf8_lossy(&script));
            Ok(ExitCode::SUCCESS)
        }

        Some(("shell-init", sub)) => {
            let target = sub
                .get_one::<String>("shell")
                .expect("clap enforces that shell is present");
            let (resolved, _) = resolve(matches, &config::CliOverrides::default())?;
            ui.data(&format!("{}\n", shell::init(&resolved.prompt, target)?));
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
                    _ => report::render_resolved(&resolved, &platform, ui.theme()),
                });
                Ok(ExitCode::SUCCESS)
            }
            _ => unreachable!("clap requires a config subcommand"),
        },

        Some(("bundles", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            ui.data(&match ui.format() {
                Format::Json => json::to_string(&json::bundles_view(&resolved, &platform)),
                _ => report::render_bundles(&resolved, &platform, detail(sub), ui.theme()),
            });
            Ok(ExitCode::SUCCESS)
        }

        Some(("status", sub)) => {
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            // Status never bootstraps, so managers that are absent simply
            // show their packages as unavailable - which is the truth.
            let snapshot = execute::snapshot(&platform, &[], ui)?;
            let built = plan::build(&resolved, &platform, &snapshot);
            ui.data(&match ui.format() {
                Format::Json => json::to_string(&json::status_view(&built, &platform)),
                _ => report::render_status(&built, &platform, detail(sub), ui.theme()),
            });
            Ok(ExitCode::SUCCESS)
        }

        Some(("apply", sub)) => {
            let dry_run = sub.get_flag("dry-run");
            let (resolved, platform) = resolve(matches, &cli::overrides_from(sub))?;
            refuse_root()?;

            // Phase 1: bootstrap the managers themselves. A dry run only
            // plans it; a real run does it before the snapshot, so the
            // snapshot sees what it just made available.
            let (bootstrap, becomes_available) = plan::bootstrap(&platform, execute::probe());
            let assume: &[_] = if dry_run { &becomes_available } else { &[] };
            if !dry_run && !bootstrap.is_empty() {
                ui.line("Bootstrapping package managers.");
                execute::run_commands(&bootstrap, ui)?;
            }

            // Phase 2: snapshot and plan.
            let snapshot = execute::snapshot(&platform, assume, ui)?;
            let mut built = plan::build(&resolved, &platform, &snapshot);
            if dry_run {
                built.bootstrap = bootstrap;
            }

            let home = std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .context("HOME is not set; nt needs it to find the chezmoi source directory")?;
            let source = dotfiles::source_dir(std::path::Path::new(&home));
            built.dotfiles = dotfiles::plan(
                &resolved.dotfiles,
                source.exists(),
                privilege::scripts_use_sudo(&source),
            )?;

            let json_mode = ui.format() == Format::Json;

            // Phase 3: converge, or show what converging would do.
            if dry_run {
                ui.data(&if json_mode {
                    json::to_string(&json::plan_view(&built, &platform, true))
                } else {
                    report::render_plan(&built, true, ui.theme())
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
                        "plan": json::plan_view(&built, &platform, false),
                        "run": json::report_view(&report),
                    })));
                } else {
                    if built.is_empty() {
                        ui.data(&format!(
                            "{} {}\n",
                            ui.theme().satisfied_icon(),
                            ui.theme().good.apply_to("Nothing to do.")
                        ));
                    }
                    ui.summary(&report);
                    // Notes are shown whether or not anything ran, so an
                    // unprovisionable package is never silently dropped.
                    ui.data(&report::render_notes(&built, ui.theme()));
                }
            }

            if resolved.strict && !built.unavailable().is_empty() {
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
    tracing::debug!(?detected, %hostname, config = %path.display(), "resolving");
    let resolved = config::resolve(&file, &hostname, overrides)
        .with_context(|| format!("failed to resolve configuration from {}", path.display()))?;
    Ok((resolved, detected))
}
