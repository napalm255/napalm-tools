//! Rendering plans and configuration as text.
//!
//! Kept separate from execution so `--dry-run` shows exactly the plan that a
//! real run would carry out.

use std::fmt::Write as _;

use crate::bundles::BUNDLES;
use crate::config::Resolved;
use crate::plan::ActionPlan;
use crate::platform::Platform;
use crate::ui::theme::Theme;

/// Render a plan for display.
pub fn render_plan(plan: &ActionPlan, dry_run: bool, theme: &Theme) -> String {
    let mut out = String::new();

    if dry_run {
        let _ = writeln!(
            out,
            "{}",
            theme.dim.apply_to("Dry run - no changes will be made.")
        );
    }

    if plan.is_empty() {
        let _ = writeln!(out, "Nothing to do.");
    } else {
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

    out.push_str(&render_notes(plan, theme));
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
        let _ = writeln!(out, "\n{}", theme.heading.apply_to("Skipped:"));
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

    if !plan.unavailable.is_empty() {
        let _ = writeln!(out, "\n{}", theme.heading.apply_to("Unavailable:"));
        for u in &plan.unavailable {
            let _ = writeln!(
                out,
                "  {} {} ({}): {}",
                theme.warn.apply_to("!"),
                theme.name.apply_to(&u.package),
                u.source,
                theme.dim.apply_to(&u.reason)
            );
        }
    }

    out
}

/// Render the bundle catalog with each bundle's effective state.
pub fn render_bundles(resolved: &Resolved, platform: &Platform, theme: &Theme) -> String {
    let mut out = String::new();
    for b in BUNDLES {
        let enabled = resolved.bundle_enabled(b.name);
        let applicable = b.platforms.matches(platform);
        let state = match (enabled, applicable) {
            (true, true) => "on",
            (true, false) => "on (n/a here)",
            (false, _) => "off",
        };
        let styled_state = if enabled && applicable {
            theme.good.apply_to(state).to_string()
        } else if enabled {
            theme.warn.apply_to(state).to_string()
        } else {
            theme.dim.apply_to(state).to_string()
        };
        // Pad on the unstyled text, since escape sequences have no width.
        let pad = " ".repeat(14usize.saturating_sub(state.len()));
        let _ = writeln!(
            out,
            "{:<16} {}{} {} {}",
            theme.name.apply_to(b.name),
            styled_state,
            pad,
            b.description,
            theme
                .dim
                .apply_to(format!("({} packages)", b.packages.len()))
        );
    }
    out
}

/// Render the resolved configuration.
pub fn render_resolved(resolved: &Resolved, platform: &Platform, theme: &Theme) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "platform: fedora_family={} atomic={} wsl={}",
        platform.fedora_family, platform.atomic, platform.wsl
    );
    let _ = writeln!(out, "upgrade:  {}", resolved.upgrade);
    let _ = writeln!(out, "strict:   {}", resolved.strict);
    let _ = writeln!(
        out,
        "dotfiles: enabled={} apply={} repo={}",
        resolved.dotfiles.enabled,
        resolved.dotfiles.apply,
        resolved.dotfiles.repo.as_deref().unwrap_or("<unset>")
    );
    let _ = writeln!(out, "{}", theme.heading.apply_to("bundles:"));
    for (name, on) in &resolved.bundles {
        let state = if *on { "on" } else { "off" };
        let styled = if *on {
            theme.good.apply_to(state).to_string()
        } else {
            theme.dim.apply_to(state).to_string()
        };
        let _ = writeln!(out, "  {:<16} {styled}", theme.name.apply_to(name));
    }
    if !resolved.extra.is_empty() {
        let _ = writeln!(out, "{}", theme.heading.apply_to("extra:"));
        for (manager, packages) in &resolved.extra {
            let _ = writeln!(out, "  {manager:<16} {}", packages.join(" "));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managers::ManagerId;
    use crate::plan::{Action, SkippedBundle, Unavailable};

    fn plan_with(actions: Vec<Action>) -> ActionPlan {
        ActionPlan {
            actions,
            ..Default::default()
        }
    }

    #[test]
    fn an_empty_plan_says_there_is_nothing_to_do() {
        let out = render_plan(&ActionPlan::default(), false, &Theme::plain());

        assert!(out.to_lowercase().contains("nothing to do"), "got: {out}");
    }

    #[test]
    fn a_dry_run_is_labelled_as_one() {
        let out = render_plan(&ActionPlan::default(), true, &Theme::plain());

        assert!(out.to_lowercase().contains("dry run"), "got: {out}");
    }

    #[test]
    fn a_real_run_is_not_labelled_a_dry_run() {
        let out = render_plan(&ActionPlan::default(), false, &Theme::plain());

        assert!(!out.to_lowercase().contains("dry run"), "got: {out}");
    }

    #[test]
    fn each_action_is_shown_as_the_command_it_will_run() {
        let out = render_plan(
            &plan_with(vec![Action::Install {
                manager: ManagerId::Brew,
                packages: vec!["bat".into(), "fd".into()],
            }]),
            true,
            &Theme::plain(),
        );

        assert!(out.contains("brew install bat fd"), "got: {out}");
    }

    #[test]
    fn unavailable_packages_are_reported_with_their_reason() {
        let plan = ActionPlan {
            unavailable: vec![Unavailable {
                package: "xdotool".into(),
                source: "desktop".into(),
                reason: "no user-space provider on an atomic host".into(),
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, true, &Theme::plain());

        assert!(out.contains("xdotool"), "got: {out}");
        assert!(out.contains("no user-space provider"), "got: {out}");
        assert!(out.contains("desktop"), "should name the bundle: {out}");
    }

    #[test]
    fn skipped_bundles_are_reported() {
        let plan = ActionPlan {
            skipped: vec![SkippedBundle {
                name: "desktop".into(),
                reason: "not applicable to this platform".into(),
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, true, &Theme::plain());

        assert!(out.contains("desktop"), "got: {out}");
        assert!(out.contains("not applicable"), "got: {out}");
    }

    #[test]
    fn a_plan_with_only_unavailable_packages_is_still_nothing_to_do() {
        // Nothing will be changed, but the reason must still be visible.
        let plan = ActionPlan {
            unavailable: vec![Unavailable {
                package: "xdotool".into(),
                source: "desktop".into(),
                reason: "no user-space provider on an atomic host".into(),
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, false, &Theme::plain());

        assert!(out.to_lowercase().contains("nothing to do"), "got: {out}");
        assert!(out.contains("xdotool"), "got: {out}");
    }

    #[test]
    fn every_bundle_appears_in_the_bundle_listing() {
        let resolved = crate::config::resolve(
            &crate::config::ConfigFile::default(),
            "testhost",
            &crate::config::CliOverrides::default(),
        )
        .unwrap();
        let platform = Platform {
            fedora_family: true,
            atomic: true,
            wsl: false,
        };

        let out = render_bundles(&resolved, &platform, &Theme::plain());

        for b in BUNDLES {
            assert!(out.contains(b.name), "missing {} in: {out}", b.name);
        }
    }

    #[test]
    fn a_bundle_inapplicable_to_this_host_is_marked_as_such() {
        let resolved = crate::config::resolve(
            &crate::config::ConfigFile::parse("[bundles]\ndesktop = true\n").unwrap(),
            "testhost",
            &crate::config::CliOverrides::default(),
        )
        .unwrap();
        let wsl = Platform {
            fedora_family: true,
            atomic: false,
            wsl: true,
        };

        let out = render_bundles(&resolved, &wsl, &Theme::plain());

        let line = out.lines().find(|l| l.starts_with("desktop")).unwrap();
        assert!(line.contains("n/a"), "got: {line}");
    }

    #[test]
    fn the_resolved_view_shows_platform_and_every_bundle() {
        let resolved = crate::config::resolve(
            &crate::config::ConfigFile::default(),
            "testhost",
            &crate::config::CliOverrides::default(),
        )
        .unwrap();
        let platform = Platform {
            fedora_family: true,
            atomic: true,
            wsl: false,
        };

        let out = render_resolved(&resolved, &platform, &Theme::plain());

        assert!(out.contains("atomic=true"), "got: {out}");
        for b in BUNDLES {
            assert!(out.contains(b.name), "missing {} in: {out}", b.name);
        }
    }

    #[test]
    fn dotfile_commands_are_listed_with_the_other_actions() {
        let plan = ActionPlan {
            dotfiles: vec![crate::managers::Cmd::new("chezmoi", ["apply"])],
            ..Default::default()
        };

        let out = render_plan(&plan, true, &Theme::plain());

        assert!(out.contains("chezmoi apply"), "got: {out}");
        assert!(
            !out.to_lowercase().contains("nothing to do"),
            "a dotfiles step is something to do: {out}"
        );
    }

    #[test]
    fn dotfile_commands_come_before_the_unavailable_section() {
        // Otherwise they read as part of the unavailable list.
        let plan = ActionPlan {
            dotfiles: vec![crate::managers::Cmd::new("chezmoi", ["apply"])],
            unavailable: vec![Unavailable {
                package: "xdotool".into(),
                source: "desktop".into(),
                reason: "no user-space provider on an atomic host".into(),
            }],
            ..Default::default()
        };

        let out = render_plan(&plan, true, &Theme::plain());
        let dotfiles_at = out.find("chezmoi apply").expect("dotfiles line");
        let unavailable_at = out.find("Unavailable:").expect("unavailable section");

        assert!(
            dotfiles_at < unavailable_at,
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
            unavailable: vec![Unavailable {
                package: "xdotool".into(),
                source: "desktop".into(),
                reason: "no user-space provider on an atomic host".into(),
            }],
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

        assert!(
            render_notes(&plan, &Theme::plain()).is_empty(),
            "got: {:?}",
            render_notes(&plan, &Theme::plain())
        );
    }

    #[test]
    fn notes_report_skipped_bundles() {
        let plan = ActionPlan {
            skipped: vec![SkippedBundle {
                name: "desktop".into(),
                reason: "not applicable to this platform".into(),
            }],
            ..Default::default()
        };

        assert!(render_notes(&plan, &Theme::plain()).contains("desktop"));
    }
}
