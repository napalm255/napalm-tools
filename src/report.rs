//! Rendering plans, status and configuration as text.
//!
//! Kept separate from execution so `--dry-run` shows exactly the plan that a
//! real run would carry out.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use console::measure_text_width;

use crate::bundles::{BUNDLES, Bundle};
use crate::config::Resolved;
use crate::plan::{ActionPlan, PackageState, State};
use crate::platform::Platform;
use crate::ui::theme::Theme;

/// Pad `styled` (which may contain escapes) to `width` visible columns.
fn pad(styled: &str, width: usize) -> String {
    let visible = measure_text_width(styled);
    format!("{styled}{}", " ".repeat(width.saturating_sub(visible)))
}

/// Width of the bundle-name column: the longest catalog name, so a new
/// bundle widens the column instead of breaking the alignment.
fn bundle_width() -> usize {
    widest(BUNDLES.iter().map(|b| b.name))
}

/// Width of the state column in the catalog listing.
const STATE_WIDTH: usize = "n/a here".len();

/// The longest of `names`, as a column width.
fn widest<'a>(names: impl Iterator<Item = &'a str>) -> usize {
    names.map(str::len).max().unwrap_or(0)
}

/// Render a plan for display.
///
/// The "dry run" advisory is not part of this: it is commentary, and goes
/// to stderr, so `--output plain > plan.txt` holds only the plan.
pub fn render_plan(plan: &ActionPlan, theme: &Theme) -> String {
    let mut out = String::new();

    if plan.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            theme.with_icon(
                &theme.satisfied_icon(),
                &theme.good.apply_to("Nothing to do.").to_string()
            )
        );
    } else {
        if !plan.bootstrap.is_empty() {
            let _ = writeln!(
                out,
                "{}",
                theme.heading_line(&theme.bootstrap_icon(), "Bootstrap:")
            );
            for cmd in &plan.bootstrap {
                let _ = writeln!(
                    out,
                    "  {} {}",
                    theme.good.apply_to("+"),
                    theme.name.apply_to(cmd.to_shell())
                );
            }
            let _ = writeln!(out);
        }
        let missing = plan.missing();
        let title = if missing.is_empty() {
            "Steps:".to_string()
        } else {
            format!(
                "Steps ({} package{} to install):",
                missing.len(),
                if missing.len() == 1 { "" } else { "s" }
            )
        };
        let _ = writeln!(out, "{}", theme.heading_line(&theme.install_icon(), &title));
        for action in &plan.actions {
            let _ = writeln!(
                out,
                "  {} {}",
                theme.good.apply_to("+"),
                theme.name.apply_to(action.to_cmd().to_shell())
            );
        }
        for cmd in &plan.dotfiles {
            let _ = writeln!(
                out,
                "  {} {}",
                theme.good.apply_to("+"),
                theme.name.apply_to(cmd.to_shell())
            );
        }
    }

    let notes = render_notes(plan, theme);
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&notes);
    }
    out
}

/// Render only the advisory sections of a plan - what was skipped and what
/// could not be provisioned.
///
/// Used after a real run, where the actions have already been echoed as they
/// executed and repeating them would just be noise.
pub fn render_notes(plan: &ActionPlan, theme: &Theme) -> String {
    let mut out = String::new();

    if !plan.skipped.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            theme.heading_line(&theme.skip_icon(), "Skipped:")
        );
        for s in &plan.skipped {
            let _ = writeln!(
                out,
                "  {} {}: {}",
                theme.dim.apply_to("-"),
                theme.name.apply_to(&s.name),
                theme.dim.apply_to(&s.reason)
            );
        }
    }

    let unavailable = plan.unavailable();
    if !unavailable.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "{}",
            theme.heading_line(&theme.warn_icon(), "Unavailable:")
        );
        for u in &unavailable {
            let _ = writeln!(
                out,
                "  {} {} ({}): {}",
                theme.unavailable_mark(),
                theme.name.apply_to(&u.package),
                u.source,
                theme.dim.apply_to(&u.reason)
            );
        }
    }

    out
}

/// One bundle's line in the catalog listing: name, state, description.
fn bundle_line(b: &Bundle, resolved: &Resolved, platform: &Platform, theme: &Theme) -> String {
    let enabled = resolved.bundle_enabled(b.name);
    let rejection = b.platforms.rejection(platform);
    let styled = match (enabled, rejection) {
        (true, None) => theme.good.apply_to("on").to_string(),
        (true, Some(_)) => theme.warn.apply_to("n/a here").to_string(),
        (false, _) => theme.dim.apply_to("off").to_string(),
    };
    let count = b.wanted(&resolved.prompt).count();
    format!(
        "{} {} {} {}",
        pad(&theme.name.apply_to(b.name).to_string(), bundle_width()),
        pad(&styled, STATE_WIDTH),
        b.description,
        theme.dim.apply_to(format!(
            "({count} package{})",
            if count == 1 { "" } else { "s" }
        ))
    )
}

/// Render the bundle catalog with each bundle's effective state. With
/// `detail`, every package and the provider it would come from.
pub fn render_bundles(
    resolved: &Resolved,
    platform: &Platform,
    detail: bool,
    theme: &Theme,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}",
        theme.heading_line(&theme.bundle_icon(), "Bundles")
    );
    let package_width = widest(
        BUNDLES
            .iter()
            .flat_map(|b| b.wanted(&resolved.prompt))
            .map(|p| p.name),
    );
    for b in BUNDLES {
        let _ = writeln!(out, "{}", bundle_line(b, resolved, platform, theme));
        if detail {
            for pkg in b.wanted(&resolved.prompt) {
                let providers: Vec<String> = pkg
                    .providers
                    .iter()
                    .map(|p| {
                        let mut s =
                            format!("{}:{}", theme.manager.apply_to(p.manager.as_str()), p.id);
                        if let Some(tap) = p.tap {
                            let _ = write!(s, " (tap {tap})");
                        }
                        if let Some(reason) = p.platforms.rejection(platform) {
                            let _ = write!(s, " [{reason}]");
                        }
                        s
                    })
                    .collect();
                let _ = writeln!(
                    out,
                    "    {} {}",
                    pad(&theme.name.apply_to(pkg.name).to_string(), package_width),
                    theme.dim.apply_to(providers.join(", "))
                );
            }
        }
    }
    out
}

/// Counts of package states within one bundle.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    /// Installed by the chosen manager or present on `PATH`.
    pub present: usize,
    /// An action would install it.
    pub missing: usize,
    /// No provider here.
    pub unavailable: usize,
}

impl Tally {
    fn add(&mut self, state: &State) {
        match state {
            State::Installed | State::OnPath => self.present += 1,
            State::Missing => self.missing += 1,
            State::Unavailable(_) => self.unavailable += 1,
        }
    }
}

/// Tally the plan's packages per bundle, in catalog order with `extra` last.
pub fn tally(plan: &ActionPlan) -> Vec<(String, Tally)> {
    let mut by_bundle: BTreeMap<&str, Tally> = BTreeMap::new();
    for p in &plan.packages {
        by_bundle.entry(&p.bundle).or_default().add(&p.state);
    }
    let mut ordered: Vec<(String, Tally)> = BUNDLES
        .iter()
        .filter_map(|b| by_bundle.remove(b.name).map(|t| (b.name.to_string(), t)))
        .collect();
    if let Some(t) = by_bundle.remove("extra") {
        ordered.push(("extra".to_string(), t));
    }
    ordered
}

/// Where a package was found, for the detail view.
fn state_text(p: &PackageState, theme: &Theme) -> String {
    let via = |m: &str| theme.manager.apply_to(m).to_string();
    match (&p.state, &p.provider) {
        (State::Installed, Some((m, id))) => {
            format!(
                "{} {} {}",
                theme.present_mark(),
                via(m.as_str()),
                theme.dim.apply_to(id)
            )
        }
        (State::Installed, None) => theme.present_mark(),
        (State::OnPath, _) => format!("{} {}", theme.present_mark(), theme.dim.apply_to("on PATH")),
        (State::Missing, Some((m, id))) => format!(
            "{} {} {} {}",
            theme.missing_mark(),
            theme.warn.apply_to("missing"),
            via(m.as_str()),
            theme.dim.apply_to(id)
        ),
        (State::Missing, None) => format!("{} missing", theme.missing_mark()),
        (State::Unavailable(reason), _) => {
            format!(
                "{} {}",
                theme.unavailable_mark(),
                theme.dim.apply_to(reason)
            )
        }
    }
}

/// Render desired-versus-installed state, per bundle. With `detail`, every
/// package and where it was found.
pub fn render_status(
    plan: &ActionPlan,
    platform: &Platform,
    detail: bool,
    theme: &Theme,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {}",
        theme.platform_icon(),
        theme.dim.apply_to(platform_summary(platform))
    );
    let _ = writeln!(
        out,
        "{}",
        theme.heading_line(&theme.bundle_icon(), "Bundles")
    );

    let package_width = widest(plan.packages.iter().map(|p| p.name.as_str()));
    let mut total = Tally::default();
    for (bundle, t) in tally(plan) {
        total.present += t.present;
        total.missing += t.missing;
        total.unavailable += t.unavailable;
        let mut parts = vec![
            theme
                .good
                .apply_to(format!("{} present", t.present))
                .to_string(),
        ];
        if t.missing > 0 {
            parts.push(
                theme
                    .warn
                    .apply_to(format!("{} missing", t.missing))
                    .to_string(),
            );
        }
        if t.unavailable > 0 {
            parts.push(
                theme
                    .bad
                    .apply_to(format!("{} unavailable", t.unavailable))
                    .to_string(),
            );
        }
        let _ = writeln!(
            out,
            "{} {}",
            pad(&theme.name.apply_to(&bundle).to_string(), bundle_width()),
            parts.join(theme.dim.apply_to(", ").to_string().as_str())
        );
        if detail {
            for p in plan.packages.iter().filter(|p| p.bundle == bundle) {
                let _ = writeln!(
                    out,
                    "    {} {}",
                    pad(&theme.name.apply_to(&p.name).to_string(), package_width),
                    state_text(p, theme)
                );
            }
        }
    }
    for s in &plan.skipped {
        let _ = writeln!(
            out,
            "{} {}",
            pad(&theme.name.apply_to(&s.name).to_string(), bundle_width()),
            theme.dim.apply_to(format!("skipped: {}", s.reason))
        );
    }

    let _ = writeln!(out);
    let mut summary = format!(
        "{} of {} packages present",
        total.present,
        total.present + total.missing + total.unavailable
    );
    if total.missing > 0 {
        let _ = write!(summary, ", {} to install", total.missing);
    }
    if total.unavailable > 0 {
        let _ = write!(summary, ", {} unavailable", total.unavailable);
    }
    let icon = if total.missing == 0 && total.unavailable == 0 {
        theme.satisfied_icon()
    } else {
        theme.install_icon()
    };
    let _ = writeln!(out, "{}", theme.heading_line(&icon, &summary));
    if total.missing > 0 {
        let _ = writeln!(
            out,
            "{}",
            theme
                .dim
                .apply_to("Run `nt apply` to install what is missing.")
        );
    }
    out
}

/// A one-line description of the platform.
pub fn platform_summary(platform: &Platform) -> String {
    let mut parts = vec![if platform.fedora_family {
        "fedora"
    } else {
        "not fedora"
    }];
    if platform.atomic {
        parts.push("atomic");
    }
    if platform.wsl {
        parts.push("wsl");
    }
    if platform.container {
        parts.push("container");
    }
    parts.push(if platform.graphical {
        "graphical"
    } else {
        "headless"
    });
    parts.join(", ")
}

/// Render the resolved configuration.
pub fn render_resolved(resolved: &Resolved, platform: &Platform, theme: &Theme) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} platform: {}",
        theme.platform_icon(),
        platform_summary(platform)
    );
    let _ = writeln!(out, "prompt:   {}", resolved.prompt);
    let _ = writeln!(out, "upgrade:  {}", resolved.upgrade);
    let _ = writeln!(out, "strict:   {}", resolved.strict);
    let _ = writeln!(out, "update:   check={}", resolved.update_check);
    let _ = writeln!(
        out,
        "dotfiles: enabled={} apply={} repo={}",
        resolved.dotfiles.enabled,
        resolved.dotfiles.apply,
        resolved.dotfiles.repo.as_deref().unwrap_or("<unset>")
    );
    let _ = writeln!(
        out,
        "{}",
        theme.heading_line(&theme.bundle_icon(), "bundles:")
    );
    for (name, on) in &resolved.bundles {
        let styled = if *on {
            theme.good.apply_to("on").to_string()
        } else {
            theme.dim.apply_to("off").to_string()
        };
        let _ = writeln!(
            out,
            "  {} {styled}",
            pad(&theme.name.apply_to(name).to_string(), bundle_width())
        );
    }
    if !resolved.extra.is_empty() {
        let _ = writeln!(out, "{}", theme.heading.apply_to("extra:"));
        for (manager, packages) in &resolved.extra {
            let _ = writeln!(out, "  {manager:<12} {}", packages.join(" "));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CliOverrides, ConfigFile, resolve};
    use crate::managers::{Cmd, ManagerId};
    use crate::plan::{Action, SkippedBundle};
    use crate::platform::test_platforms::*;

    fn plan_with(actions: Vec<Action>) -> ActionPlan {
        ActionPlan {
            actions,
            ..Default::default()
        }
    }

    fn unavailable(name: &str, bundle: &str, reason: &str) -> PackageState {
        PackageState {
            bundle: bundle.into(),
            name: name.into(),
            provider: None,
            state: State::Unavailable(reason.into()),
        }
    }

    fn resolved(text: &str) -> Resolved {
        resolve(
            &ConfigFile::parse(text).unwrap(),
            "testhost",
            &CliOverrides::default(),
        )
        .unwrap()
    }

    #[test]
    fn an_empty_plan_says_there_is_nothing_to_do() {
        let out = render_plan(&ActionPlan::default(), &Theme::plain());

        assert!(out.to_lowercase().contains("nothing to do"), "got: {out}");
    }

    #[test]
    fn the_plan_carries_no_dry_run_advisory() {
        // The advisory is commentary and belongs on stderr; the plan on
        // stdout must be only the plan.
        let out = render_plan(&ActionPlan::default(), &Theme::plain());

        assert!(!out.to_lowercase().contains("dry run"), "got: {out}");
    }

    #[test]
    fn each_action_is_shown_as_the_command_it_will_run() {
        let out = render_plan(
            &plan_with(vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into(), "fd".into()],
            }]),
            &Theme::plain(),
        );

        assert!(out.contains("brew install bat fd"), "got: {out}");
    }

    #[test]
    fn bootstrap_steps_are_listed_first_under_their_own_heading() {
        let plan = ActionPlan {
            bootstrap: vec![Cmd::new("brew", ["install", "mise"])],
            actions: vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into()],
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, &Theme::plain());

        assert!(out.contains("Bootstrap:"), "got: {out}");
        assert!(out.find("brew install mise") < out.find("brew install bat"));
    }

    #[test]
    fn unavailable_packages_are_reported_with_their_reason() {
        let plan = ActionPlan {
            packages: vec![unavailable(
                "xdotool",
                "desktop",
                "no user-space provider on an atomic host",
            )],
            ..Default::default()
        };

        let out = render_plan(&plan, &Theme::plain());

        assert!(out.contains("xdotool"), "got: {out}");
        assert!(out.contains("no user-space provider"), "got: {out}");
        assert!(out.contains("desktop"), "should name the bundle: {out}");
        assert!(out.to_lowercase().contains("nothing to do"), "got: {out}");
    }

    #[test]
    fn skipped_bundles_are_reported() {
        let plan = ActionPlan {
            skipped: vec![SkippedBundle {
                name: "desktop".into(),
                reason: "needs a desktop session; this is WSL".into(),
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, &Theme::plain());

        assert!(out.contains("desktop"), "got: {out}");
        assert!(out.contains("needs a desktop"), "got: {out}");
    }

    #[test]
    fn every_bundle_appears_in_the_bundle_listing() {
        let out = render_bundles(&resolved(""), &ATOMIC, false, &Theme::plain());

        for b in BUNDLES {
            assert!(out.contains(b.name), "missing {} in: {out}", b.name);
        }
        assert!(!out.contains("ripgrep"), "packages need --detail: {out}");
    }

    #[test]
    fn detail_lists_every_package_with_its_providers() {
        let out = render_bundles(&resolved(""), &PLAIN, true, &Theme::plain());

        assert!(out.contains("ripgrep"), "got: {out}");
        assert!(out.contains("brew:ripgrep"), "got: {out}");
        assert!(out.contains("mise:java@corretto-21"), "got: {out}");
        assert!(out.contains("tap powertmux/powertmux"), "got: {out}");
    }

    #[test]
    fn detail_shows_only_the_selected_prompt() {
        let out = render_bundles(
            &resolved("[shell]\nprompt = \"powerbash\"\n"),
            &PLAIN,
            true,
            &Theme::plain(),
        );

        assert!(out.contains("powerbash"), "got: {out}");
        assert!(!out.contains("starship"), "got: {out}");
    }

    #[test]
    fn a_bundle_inapplicable_to_this_host_is_marked_as_such() {
        let out = render_bundles(&resolved(""), &UNDER_WSL, false, &Theme::plain());

        let line = out.lines().find(|l| l.starts_with("desktop")).unwrap();
        assert!(line.contains("n/a"), "got: {line}");
    }

    #[test]
    fn a_disabled_bundle_is_off() {
        let out = render_bundles(
            &resolved("[bundles]\nandroid = false\n"),
            &PLAIN,
            false,
            &Theme::plain(),
        );

        let line = out.lines().find(|l| l.starts_with("android")).unwrap();
        assert!(line.contains("off"), "got: {line}");
    }

    #[test]
    fn the_resolved_view_shows_platform_prompt_and_every_bundle() {
        let out = render_resolved(&resolved(""), &ATOMIC, &Theme::plain());

        assert!(out.contains("atomic"), "got: {out}");
        assert!(out.contains("prompt:   starship"), "got: {out}");
        for b in BUNDLES {
            assert!(out.contains(b.name), "missing {} in: {out}", b.name);
        }
    }

    #[test]
    fn dotfile_commands_are_listed_with_the_other_actions() {
        let plan = ActionPlan {
            dotfiles: vec![Cmd::new("chezmoi", ["apply"])],
            packages: vec![unavailable("x", "desktop", "why")],
            ..Default::default()
        };

        let out = render_plan(&plan, &Theme::plain());

        assert!(out.contains("chezmoi apply"), "got: {out}");
        assert!(!out.to_lowercase().contains("nothing to do"), "{out}");
        assert!(
            out.find("chezmoi apply") < out.find("Unavailable:"),
            "dotfiles must be listed as an action, not after the notes:\n{out}"
        );
    }

    #[test]
    fn notes_omit_the_actions_that_already_ran() {
        let plan = ActionPlan {
            actions: vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into()],
            }],
            packages: vec![unavailable("xdotool", "desktop", "why")],
            ..Default::default()
        };

        let out = render_notes(&plan, &Theme::plain());

        assert!(
            !out.contains("brew install"),
            "actions must not repeat: {out}"
        );
        assert!(out.contains("xdotool"), "got: {out}");
    }

    #[test]
    fn notes_are_empty_when_there_is_nothing_to_flag() {
        let plan = plan_with(vec![Action::Install {
            manager: ManagerId::Brew,
            packages: vec!["bat".into()],
        }]);

        assert!(render_notes(&plan, &Theme::plain()).is_empty());
    }

    fn status_plan() -> ActionPlan {
        ActionPlan {
            packages: vec![
                PackageState {
                    bundle: "core".into(),
                    name: "ripgrep".into(),
                    provider: Some((ManagerId::Brew, "ripgrep".into())),
                    state: State::Installed,
                },
                PackageState {
                    bundle: "core".into(),
                    name: "vim".into(),
                    provider: Some((ManagerId::Brew, "vim".into())),
                    state: State::OnPath,
                },
                PackageState {
                    bundle: "core".into(),
                    name: "fd".into(),
                    provider: Some((ManagerId::Brew, "fd".into())),
                    state: State::Missing,
                },
                PackageState {
                    bundle: "java".into(),
                    name: "java".into(),
                    provider: Some((ManagerId::Mise, "java@corretto-21".into())),
                    state: State::Missing,
                },
                unavailable(
                    "some-kernel-tool",
                    "extra",
                    "dnf is not available on this host",
                ),
            ],
            skipped: vec![SkippedBundle {
                name: "fonts".into(),
                reason: "needs a desktop session; this is a container".into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn status_tallies_each_bundle_in_catalog_order_with_extra_last() {
        let t = tally(&status_plan());

        assert_eq!(t[0].0, "core");
        assert_eq!(
            t[0].1,
            Tally {
                present: 2,
                missing: 1,
                unavailable: 0
            }
        );
        assert_eq!(t[1].0, "java");
        assert_eq!(t[2].0, "extra");
        assert_eq!(t[2].1.unavailable, 1);
    }

    #[test]
    fn status_summarises_and_points_at_apply() {
        let out = render_status(&status_plan(), &CONTAINER, false, &Theme::plain());

        assert!(out.contains("2 of 5 packages present"), "got: {out}");
        assert!(out.contains("2 to install"), "got: {out}");
        assert!(out.contains("1 unavailable"), "got: {out}");
        assert!(out.contains("nt apply"), "got: {out}");
        assert!(out.contains("fonts"), "skipped bundles are listed: {out}");
        assert!(out.contains("container"), "platform is shown: {out}");
        assert!(!out.contains("ripgrep"), "packages need --detail: {out}");
    }

    #[test]
    fn status_detail_shows_where_each_package_was_found() {
        let out = render_status(&status_plan(), &PLAIN, true, &Theme::plain());

        // Package rows are indented; bundle rows are not.
        let line = |name: &str| {
            out.lines()
                .find(|l| l.starts_with("    ") && l.trim_start().starts_with(name))
                .unwrap_or_else(|| panic!("no row for {name} in:\n{out}"))
                .to_string()
        };
        assert!(line("ripgrep").contains("brew"), "{}", line("ripgrep"));
        assert!(line("vim").contains("on PATH"), "{}", line("vim"));
        assert!(line("fd").contains("missing"), "{}", line("fd"));
        assert!(line("java").contains("mise") && line("java").contains("corretto"));
        assert!(line("some-kernel-tool").contains("dnf is not available"));
    }

    #[test]
    fn a_converged_status_says_so_without_telling_you_to_apply() {
        let plan = ActionPlan {
            packages: vec![PackageState {
                bundle: "core".into(),
                name: "ripgrep".into(),
                provider: Some((ManagerId::Brew, "ripgrep".into())),
                state: State::Installed,
            }],
            ..Default::default()
        };

        let out = render_status(&plan, &PLAIN, false, &Theme::plain());

        assert!(out.contains("1 of 1 packages present"), "got: {out}");
        assert!(!out.contains("nt apply"), "got: {out}");
    }

    #[test]
    fn the_platform_summary_names_what_matters() {
        assert_eq!(platform_summary(&ATOMIC), "fedora, atomic, graphical");
        assert_eq!(platform_summary(&SERVER), "fedora, headless");
        assert_eq!(platform_summary(&UNDER_WSL), "fedora, wsl, headless");
        assert_eq!(platform_summary(&CONTAINER), "fedora, container, headless");
    }

    #[test]
    fn padding_ignores_escape_sequences() {
        let t = Theme::coloured();
        let styled = t.name.apply_to("abc").to_string();

        assert_eq!(measure_text_width(&pad(&styled, 6)), 6);
    }

    #[test]
    fn columns_are_as_wide_as_the_longest_name_and_no_wider() {
        // Fixed widths broke alignment the day a longer name arrived; the
        // widths now follow the catalog.
        let longest = BUNDLES.iter().map(|b| b.name.len()).max().unwrap();
        let resolved = resolved("");

        let out = render_bundles(&resolved, &PLAIN, true, &Theme::plain());

        for line in out
            .lines()
            .filter(|l| !l.starts_with(' ') && !l.starts_with("Bundles"))
        {
            let (name, rest) = line.split_at(line.find(' ').unwrap());
            let state_starts = rest.len() - rest.trim_start().len();
            assert_eq!(name.len() + state_starts, longest + 1, "{line:?}");
        }
        assert!(
            out.lines().any(|l| l.contains("lua-language-server ")),
            "the longest package name still gets one space"
        );
    }

    #[test]
    fn a_plain_theme_leaves_no_leading_space_where_an_icon_would_be() {
        let out = render_plan(&ActionPlan::default(), &Theme::plain());

        assert!(out.starts_with("Nothing to do."), "got: {out:?}");
    }
}
