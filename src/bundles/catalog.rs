//! The bundle catalog.
//!
//! This is data, not logic. Every third-party entry was checked against the
//! project's dependency rules: at least 1000 GitHub stars, a commit within six
//! months, not archived, and a compatible licence. Tooling published by a
//! language or platform owner is judged on official status instead, which is
//! why `govulncheck` (the Go team's own scanner, ~510 stars) is here.
//!
//! Three candidates were rejected by those rules and are recorded here so they
//! are not reintroduced:
//!
//! - `pup` - the original is two years stale and the maintained fork sits just
//!   under the star threshold. Replaced by `htmlq`, which does the same job.
//! - `html2text` - no commit in ten months. Replaced by `pandoc`.
//! - `tree` - an OS-native package, so the dependency ladder stops before
//!   reaching Homebrew. `eza --tree` covers it in any case.
//!
//! Note also that the Homebrew *formula* `copilot` is the AWS ECS tool and its
//! upstream is archived; GitHub Copilot is the *cask* `copilot-cli`. Formula
//! and cask names are separate namespaces, which is why they are separate
//! managers.

use super::{Bundle, Pkg, Provider};
use crate::managers::ManagerId;
use crate::platform::Platforms;

// --- core -------------------------------------------------------------------

static CORE: &[Pkg] = &[
    Pkg {
        name: "ripgrep",
        binary: Some("rg"),
        providers: &[Provider::new(ManagerId::Brew, "ripgrep")],
    },
    Pkg {
        name: "fd",
        binary: Some("fd"),
        providers: &[Provider::new(ManagerId::Brew, "fd")],
    },
    Pkg {
        name: "bat",
        binary: Some("bat"),
        providers: &[Provider::new(ManagerId::Brew, "bat")],
    },
    Pkg {
        name: "eza",
        binary: Some("eza"),
        providers: &[Provider::new(ManagerId::Brew, "eza")],
    },
    Pkg {
        name: "zoxide",
        binary: Some("zoxide"),
        providers: &[Provider::new(ManagerId::Brew, "zoxide")],
    },
    Pkg {
        name: "fzf",
        binary: Some("fzf"),
        providers: &[Provider::new(ManagerId::Brew, "fzf")],
    },
    Pkg {
        name: "jq",
        binary: Some("jq"),
        providers: &[Provider::new(ManagerId::Brew, "jq")],
    },
    Pkg {
        name: "yq",
        binary: Some("yq"),
        providers: &[Provider::new(ManagerId::Brew, "yq")],
    },
    Pkg {
        name: "sd",
        binary: Some("sd"),
        providers: &[Provider::new(ManagerId::Brew, "sd")],
    },
    Pkg {
        name: "git-delta",
        binary: Some("delta"),
        providers: &[Provider::new(ManagerId::Brew, "git-delta")],
    },
    Pkg {
        name: "hyperfine",
        binary: Some("hyperfine"),
        providers: &[Provider::new(ManagerId::Brew, "hyperfine")],
    },
    Pkg {
        name: "tealdeer",
        binary: Some("tldr"),
        providers: &[Provider::new(ManagerId::Brew, "tealdeer")],
    },
    Pkg {
        name: "vim",
        binary: Some("vim"),
        providers: &[Provider::new(ManagerId::Brew, "vim")],
    },
    Pkg {
        name: "git",
        binary: Some("git"),
        providers: &[Provider::new(ManagerId::Brew, "git")],
    },
    Pkg {
        name: "gh",
        binary: Some("gh"),
        providers: &[Provider::new(ManagerId::Brew, "gh")],
    },
    Pkg {
        name: "chezmoi",
        binary: Some("chezmoi"),
        providers: &[Provider::new(ManagerId::Brew, "chezmoi")],
    },
    Pkg {
        name: "just",
        binary: Some("just"),
        providers: &[Provider::new(ManagerId::Brew, "just")],
    },
    Pkg {
        name: "mise",
        binary: Some("mise"),
        providers: &[Provider::new(ManagerId::Brew, "mise")],
    },
    Pkg {
        name: "direnv",
        binary: Some("direnv"),
        providers: &[Provider::new(ManagerId::Brew, "direnv")],
    },
    Pkg {
        name: "watchexec",
        binary: Some("watchexec"),
        providers: &[Provider::new(ManagerId::Brew, "watchexec")],
    },
    Pkg {
        name: "tokei",
        binary: Some("tokei"),
        providers: &[Provider::new(ManagerId::Brew, "tokei")],
    },
    Pkg {
        name: "lazygit",
        binary: Some("lazygit"),
        providers: &[Provider::new(ManagerId::Brew, "lazygit")],
    },
    Pkg {
        name: "difftastic",
        binary: Some("difft"),
        providers: &[Provider::new(ManagerId::Brew, "difftastic")],
    },
    Pkg {
        name: "actionlint",
        binary: Some("actionlint"),
        providers: &[Provider::new(ManagerId::Brew, "actionlint")],
    },
    Pkg {
        name: "htop",
        binary: Some("htop"),
        providers: &[Provider::new(ManagerId::Brew, "htop")],
    },
    Pkg {
        name: "btop",
        binary: Some("btop"),
        providers: &[Provider::new(ManagerId::Brew, "btop")],
    },
    Pkg {
        name: "wget",
        binary: Some("wget"),
        providers: &[Provider::new(ManagerId::Brew, "wget")],
    },
    Pkg {
        name: "curl",
        binary: Some("curl"),
        providers: &[Provider::new(ManagerId::Brew, "curl")],
    },
    Pkg {
        name: "make",
        binary: Some("make"),
        providers: &[Provider::new(ManagerId::Brew, "make")],
    },
    Pkg {
        name: "man-db",
        binary: Some("man"),
        providers: &[Provider::new(ManagerId::Brew, "man-db")],
    },
    Pkg {
        name: "whois",
        binary: Some("whois"),
        providers: &[Provider::new(ManagerId::Brew, "whois")],
    },
    Pkg {
        name: "nmap",
        binary: Some("nmap"),
        providers: &[Provider::new(ManagerId::Brew, "nmap")],
    },
    // GNU inetutils supplies telnet. The standalone `telnet` formula is a
    // port of Apple's remote_cmds with no bottle, so it would build from
    // source on Linux.
    Pkg {
        name: "telnet",
        binary: Some("telnet"),
        providers: &[Provider::new(ManagerId::Brew, "inetutils")],
    },
    // No maintained user-space provider: the `netcat` formula is GNU netcat
    // 0.7.1, released in 2004 and dormant since. Distributions ship a current
    // one, so this resolves through the binary check on any ordinary system
    // and falls back to dnf where that is usable.
    Pkg {
        name: "netcat",
        binary: Some("nc"),
        providers: &[Provider::gated(
            ManagerId::Dnf,
            "netcat",
            Platforms::NOT_ATOMIC,
        )],
    },
    Pkg {
        name: "tmux",
        binary: Some("tmux"),
        providers: &[Provider::new(ManagerId::Brew, "tmux")],
    },
    // Third-party tap; nt runs `brew tap` before installing.
    Pkg {
        name: "powertmux",
        binary: Some("powertmux"),
        providers: &[Provider::tapped("powertmux", "powertmux/powertmux")],
    },
];

// --- shell ------------------------------------------------------------------

static SHELL: &[Pkg] = &[
    Pkg {
        name: "shellcheck",
        binary: Some("shellcheck"),
        providers: &[Provider::new(ManagerId::Brew, "shellcheck")],
    },
    Pkg {
        name: "shfmt",
        binary: Some("shfmt"),
        providers: &[Provider::new(ManagerId::Brew, "shfmt")],
    },
];

// --- security ---------------------------------------------------------------

static SECURITY: &[Pkg] = &[
    Pkg {
        name: "trivy",
        binary: Some("trivy"),
        providers: &[Provider::new(ManagerId::Brew, "trivy")],
    },
    Pkg {
        name: "gitleaks",
        binary: Some("gitleaks"),
        providers: &[Provider::new(ManagerId::Brew, "gitleaks")],
    },
    Pkg {
        name: "osv-scanner",
        binary: Some("osv-scanner"),
        providers: &[Provider::new(ManagerId::Brew, "osv-scanner")],
    },
    Pkg {
        name: "semgrep",
        binary: Some("semgrep"),
        providers: &[Provider::new(ManagerId::Brew, "semgrep")],
    },
    Pkg {
        name: "syft",
        binary: Some("syft"),
        providers: &[Provider::new(ManagerId::Brew, "syft")],
    },
    Pkg {
        name: "grype",
        binary: Some("grype"),
        providers: &[Provider::new(ManagerId::Brew, "grype")],
    },
    Pkg {
        name: "hadolint",
        binary: Some("hadolint"),
        providers: &[Provider::new(ManagerId::Brew, "hadolint")],
    },
];

// --- ai ---------------------------------------------------------------------

static AI: &[Pkg] = &[
    // Installed here by the vendor script into ~/.local/bin, where it
    // self-updates. The binary check keeps npm from adding a second, staler copy.
    Pkg {
        name: "claude-code",
        binary: Some("claude"),
        providers: &[Provider::new(ManagerId::Npm, "@anthropic-ai/claude-code")],
    },
    // The cask, not the formula: `copilot` the formula is AWS's ECS tool.
    Pkg {
        name: "copilot-cli",
        binary: Some("copilot"),
        providers: &[Provider::new(ManagerId::BrewCask, "copilot-cli")],
    },
    Pkg {
        name: "codex",
        binary: Some("codex"),
        providers: &[Provider::new(ManagerId::Npm, "@openai/codex")],
    },
    // No npm package exists; the one named `antigravity` on npm is an
    // unrelated placeholder and must not be used.
    Pkg {
        name: "antigravity-cli",
        binary: Some("antigravity"),
        providers: &[Provider::new(ManagerId::BrewCask, "antigravity-cli")],
    },
];

// --- languages --------------------------------------------------------------

static GO: &[Pkg] = &[
    Pkg {
        name: "golangci-lint",
        binary: Some("golangci-lint"),
        providers: &[Provider::new(ManagerId::Brew, "golangci-lint")],
    },
    Pkg {
        name: "govulncheck",
        binary: Some("govulncheck"),
        providers: &[Provider::new(ManagerId::Brew, "govulncheck")],
    },
    Pkg {
        name: "gopls",
        binary: Some("gopls"),
        providers: &[Provider::new(ManagerId::Brew, "gopls")],
    },
    Pkg {
        name: "goreleaser",
        binary: Some("goreleaser"),
        providers: &[Provider::new(ManagerId::Brew, "goreleaser")],
    },
];

static RUST: &[Pkg] = &[
    Pkg {
        name: "cargo-audit",
        binary: Some("cargo-audit"),
        providers: &[Provider::new(ManagerId::Brew, "cargo-audit")],
    },
    Pkg {
        name: "cargo-deny",
        binary: Some("cargo-deny"),
        providers: &[Provider::new(ManagerId::Brew, "cargo-deny")],
    },
    Pkg {
        name: "cargo-nextest",
        binary: Some("cargo-nextest"),
        providers: &[Provider::new(ManagerId::Brew, "cargo-nextest")],
    },
    Pkg {
        name: "taplo",
        binary: Some("taplo"),
        providers: &[Provider::new(ManagerId::Brew, "taplo")],
    },
    Pkg {
        name: "rust-analyzer",
        binary: Some("rust-analyzer"),
        providers: &[Provider::new(ManagerId::Brew, "rust-analyzer")],
    },
];

static PYTHON: &[Pkg] = &[
    Pkg {
        name: "ruff",
        binary: Some("ruff"),
        providers: &[Provider::new(ManagerId::Brew, "ruff")],
    },
    Pkg {
        name: "uv",
        binary: Some("uv"),
        providers: &[Provider::new(ManagerId::Brew, "uv")],
    },
    Pkg {
        name: "mypy",
        binary: Some("mypy"),
        providers: &[Provider::new(ManagerId::Brew, "mypy")],
    },
    Pkg {
        name: "pip-audit",
        binary: Some("pip-audit"),
        providers: &[Provider::new(ManagerId::Brew, "pip-audit")],
    },
    Pkg {
        name: "pipenv",
        binary: Some("pipenv"),
        providers: &[Provider::new(ManagerId::Brew, "pipenv")],
    },
];

static NODE: &[Pkg] = &[
    Pkg {
        name: "biome",
        binary: Some("biome"),
        providers: &[Provider::new(ManagerId::Brew, "biome")],
    },
    Pkg {
        name: "oxlint",
        binary: Some("oxlint"),
        providers: &[Provider::new(ManagerId::Brew, "oxlint")],
    },
    Pkg {
        name: "typescript",
        binary: Some("tsc"),
        providers: &[Provider::new(ManagerId::Brew, "typescript")],
    },
];

static WEB: &[Pkg] = &[
    Pkg {
        name: "prettier",
        binary: Some("prettier"),
        providers: &[Provider::new(ManagerId::Brew, "prettier")],
    },
    Pkg {
        name: "stylelint",
        binary: Some("stylelint"),
        providers: &[Provider::new(ManagerId::Brew, "stylelint")],
    },
    Pkg {
        name: "htmlq",
        binary: Some("htmlq"),
        providers: &[Provider::new(ManagerId::Brew, "htmlq")],
    },
    Pkg {
        name: "pandoc",
        binary: Some("pandoc"),
        providers: &[Provider::new(ManagerId::Brew, "pandoc")],
    },
    Pkg {
        name: "pa11y",
        binary: Some("pa11y"),
        providers: &[Provider::new(ManagerId::Npm, "pa11y")],
    },
];

static DATA: &[Pkg] = &[
    Pkg {
        name: "miller",
        binary: Some("mlr"),
        providers: &[Provider::new(ManagerId::Brew, "miller")],
    },
    Pkg {
        name: "duckdb",
        binary: Some("duckdb"),
        providers: &[Provider::new(ManagerId::Brew, "duckdb")],
    },
    Pkg {
        name: "qsv",
        binary: Some("qsv"),
        providers: &[Provider::new(ManagerId::Brew, "qsv")],
    },
    Pkg {
        name: "sqlite",
        binary: Some("sqlite3"),
        providers: &[Provider::new(ManagerId::Brew, "sqlite")],
    },
    Pkg {
        name: "sqlite-utils",
        binary: Some("sqlite-utils"),
        providers: &[Provider::new(ManagerId::Brew, "sqlite-utils")],
    },
];

// --- runtimes ---------------------------------------------------------------
// Off by default: mise manages runtimes on machines that use it, and two
// things managing the same runtime means PATH order decides which wins.

static GO_RUNTIME: &[Pkg] = &[Pkg {
    name: "go",
    binary: Some("go"),
    providers: &[Provider::new(ManagerId::Brew, "go")],
}];

static RUST_RUNTIME: &[Pkg] = &[Pkg {
    name: "rustup",
    binary: Some("rustup"),
    providers: &[Provider::new(ManagerId::Brew, "rustup")],
}];

static PYTHON_RUNTIME: &[Pkg] = &[Pkg {
    name: "python@3.14",
    binary: Some("python3.14"),
    providers: &[Provider::new(ManagerId::Brew, "python@3.14")],
}];

static NODE_RUNTIME: &[Pkg] = &[
    Pkg {
        name: "node",
        binary: Some("node"),
        providers: &[Provider::new(ManagerId::Brew, "node")],
    },
    Pkg {
        name: "bun",
        binary: Some("bun"),
        providers: &[Provider::new(ManagerId::Brew, "bun")],
    },
    Pkg {
        name: "pnpm",
        binary: Some("pnpm"),
        providers: &[Provider::new(ManagerId::Brew, "pnpm")],
    },
];

// --- aws --------------------------------------------------------------------

static AWS: &[Pkg] = &[
    Pkg {
        name: "awscli",
        binary: Some("aws"),
        providers: &[Provider::new(ManagerId::Brew, "awscli")],
    },
    Pkg {
        name: "aws-sam-cli",
        binary: Some("sam"),
        providers: &[Provider::new(ManagerId::Brew, "aws-sam-cli")],
    },
    Pkg {
        name: "cfn-lint",
        binary: Some("cfn-lint"),
        providers: &[Provider::new(ManagerId::Brew, "cfn-lint")],
    },
];

// --- desktop ----------------------------------------------------------------

static DESKTOP: &[Pkg] = &[
    // Flatpak applications expose no binary on PATH, so only the flatpak
    // listing can satisfy them.
    Pkg {
        name: "remmina",
        binary: None,
        providers: &[Provider::new(ManagerId::Flatpak, "org.remmina.Remmina")],
    },
    Pkg {
        name: "spotify",
        binary: None,
        providers: &[Provider::new(ManagerId::Flatpak, "com.spotify.Client")],
    },
    // No user-space provider exists on an atomic host: reported unavailable,
    // never silently installed via dnf.
    Pkg {
        name: "xdotool",
        binary: Some("xdotool"),
        providers: &[Provider::gated(
            ManagerId::Dnf,
            "xdotool",
            Platforms::NOT_ATOMIC,
        )],
    },
];

// --- fonts ------------------------------------------------------------------
// Casks with no executable; only the cask listing can satisfy them.

static FONTS: &[Pkg] = &[
    Pkg {
        name: "font-0xproto-nerd-font",
        binary: None,
        providers: &[Provider::new(ManagerId::BrewCask, "font-0xproto-nerd-font")],
    },
    Pkg {
        name: "font-blex-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-blex-mono-nerd-font",
        )],
    },
    Pkg {
        name: "font-caskaydia-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-caskaydia-mono-nerd-font",
        )],
    },
    Pkg {
        name: "font-comic-shanns-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-comic-shanns-mono-nerd-font",
        )],
    },
    Pkg {
        name: "font-droid-sans-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-droid-sans-mono-nerd-font",
        )],
    },
    Pkg {
        name: "font-fira-code-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-fira-code-nerd-font",
        )],
    },
    Pkg {
        name: "font-go-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(ManagerId::BrewCask, "font-go-mono-nerd-font")],
    },
    Pkg {
        name: "font-jetbrains-mono-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-jetbrains-mono-nerd-font",
        )],
    },
    Pkg {
        name: "font-sauce-code-pro-nerd-font",
        binary: None,
        providers: &[Provider::new(
            ManagerId::BrewCask,
            "font-sauce-code-pro-nerd-font",
        )],
    },
    Pkg {
        name: "font-source-code-pro",
        binary: None,
        providers: &[Provider::new(ManagerId::BrewCask, "font-source-code-pro")],
    },
    Pkg {
        name: "font-ubuntu-nerd-font",
        binary: None,
        providers: &[Provider::new(ManagerId::BrewCask, "font-ubuntu-nerd-font")],
    },
];

/// Every bundle `nt` knows about.
pub static BUNDLES: &[Bundle] = &[
    Bundle {
        name: "core",
        description: "Terminal and git essentials",
        default_enabled: true,
        platforms: Platforms::ALL,
        packages: CORE,
    },
    Bundle {
        name: "shell",
        description: "Shell script linting and formatting",
        default_enabled: true,
        platforms: Platforms::ALL,
        packages: SHELL,
    },
    Bundle {
        name: "security",
        description: "Vulnerability, secret and misconfiguration scanners",
        default_enabled: true,
        platforms: Platforms::ALL,
        packages: SECURITY,
    },
    Bundle {
        name: "ai",
        description: "AI coding agents and assistants",
        default_enabled: true,
        platforms: Platforms::ALL,
        packages: AI,
    },
    Bundle {
        name: "go",
        description: "Go linting, vulnerability scanning and release tooling",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: GO,
    },
    Bundle {
        name: "rust",
        description: "Rust auditing, testing and language tooling",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: RUST,
    },
    Bundle {
        name: "python",
        description: "Python linting, typing, auditing and environments",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: PYTHON,
    },
    Bundle {
        name: "node",
        description: "JavaScript and TypeScript linting and formatting",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: NODE,
    },
    Bundle {
        name: "web",
        description: "HTML, CSS and accessibility tooling",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: WEB,
    },
    Bundle {
        name: "data",
        description: "SQLite, CSV and columnar data tooling",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: DATA,
    },
    Bundle {
        name: "go-runtime",
        description: "The Go toolchain itself",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: GO_RUNTIME,
    },
    Bundle {
        name: "rust-runtime",
        description: "The Rust toolchain installer",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: RUST_RUNTIME,
    },
    Bundle {
        name: "python-runtime",
        description: "The Python interpreter itself",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: PYTHON_RUNTIME,
    },
    Bundle {
        name: "node-runtime",
        description: "Node, Bun and pnpm",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: NODE_RUNTIME,
    },
    Bundle {
        name: "aws",
        description: "AWS CLI, serverless and CloudFormation tooling",
        default_enabled: false,
        platforms: Platforms::ALL,
        packages: AWS,
    },
    Bundle {
        name: "desktop",
        description: "Graphical applications and desktop helpers",
        default_enabled: false,
        platforms: Platforms::NOT_WSL,
        packages: DESKTOP,
    },
    Bundle {
        name: "fonts",
        description: "Nerd Fonts and programming typefaces",
        default_enabled: false,
        platforms: Platforms::NOT_WSL,
        packages: FONTS,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // These guard the catalog as data. They are not driven by new behaviour;
    // they exist so a bad entry fails the build rather than a user's machine.

    #[test]
    fn bundle_names_are_unique() {
        let mut seen = HashSet::new();
        for b in BUNDLES {
            assert!(seen.insert(b.name), "duplicate bundle name: {}", b.name);
        }
    }

    #[test]
    fn bundle_names_are_valid_cli_flags() {
        for b in BUNDLES {
            assert!(
                !b.name.is_empty()
                    && b.name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && !b.name.starts_with('-')
                    && !b.name.ends_with('-'),
                "bundle name is not a usable flag: {:?}",
                b.name
            );
        }
    }

    #[test]
    fn every_package_has_at_least_one_provider() {
        for b in BUNDLES {
            for p in b.packages {
                assert!(
                    !p.providers.is_empty(),
                    "{}/{} has no providers",
                    b.name,
                    p.name
                );
            }
        }
    }

    #[test]
    fn package_names_are_unique_across_the_whole_catalog() {
        // Duplication across bundles would mean an install planned twice and
        // an ambiguous `[extra]` reference.
        let mut seen = HashSet::new();
        for b in BUNDLES {
            for p in b.packages {
                assert!(
                    seen.insert(p.name),
                    "package {} appears more than once in the catalog",
                    p.name
                );
            }
        }
    }

    #[test]
    fn taps_are_only_declared_on_brew_formula_providers() {
        for b in BUNDLES {
            for p in b.packages {
                for provider in p.providers {
                    if provider.tap.is_some() {
                        assert_eq!(
                            provider.manager,
                            ManagerId::Brew,
                            "{}/{} declares a tap on a non-formula provider",
                            b.name,
                            p.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_bundle_has_a_description() {
        for b in BUNDLES {
            assert!(!b.description.is_empty(), "{} has no description", b.name);
        }
    }

    #[test]
    fn packages_without_a_binary_are_only_fonts_and_flatpak_apps() {
        // Anything else omitting `binary` is a mistake: it would be reinstalled
        // whenever its manager happens not to know about it.
        for b in BUNDLES {
            for p in b.packages.iter().filter(|p| p.binary.is_none()) {
                let ok = p.providers.iter().all(|pr| {
                    matches!(pr.manager, ManagerId::Flatpak) || p.name.starts_with("font-")
                });
                assert!(ok, "{}/{} declares no binary", b.name, p.name);
            }
        }
    }

    #[test]
    fn the_aws_formula_is_not_confused_with_the_github_copilot_cask() {
        // `copilot` the formula is AWS's ECS tool and its upstream is archived.
        for b in BUNDLES {
            for p in b.packages {
                for pr in p.providers {
                    assert!(
                        !(pr.manager == ManagerId::Brew && pr.id == "copilot"),
                        "the brew formula `copilot` is the AWS ECS tool, not GitHub Copilot"
                    );
                }
            }
        }
    }
}
