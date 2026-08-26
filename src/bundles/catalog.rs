//! The bundle catalog.
//!
//! This is data, not logic, and it is opinionated: every bundle is on unless
//! the platform cannot host it. Each third-party entry was checked on
//! 2026-08-25 against the project's dependency rules - at least 1000 GitHub
//! stars, a push within six months, not archived, a compatible licence - with
//! the first-party carve-out for tooling published by a language or platform
//! owner (`govulncheck`, Amazon Corretto). The full vetting table lives in
//! `AGENTS.md`.
//!
//! Two managers do most of the work. Homebrew supplies command-line tools:
//! bottled for Linux, no sudo, broad coverage. mise supplies language
//! toolchains and anything that needs a JDK, pinned per user; the Kotlin,
//! Gradle and Maven formulae would each pull Homebrew's own OpenJDK in beside
//! Corretto, so they come through mise against the Corretto it installs.
//!
//! Rejected under the rules, recorded so they are not reintroduced:
//!
//! - `pup` - stale original; the maintained fork sits under the star line.
//!   `htmlq` does the job.
//! - `html2text` - no commit in ten months. `pandoc` instead.
//! - `tree` - OS-native, and `eza --tree` covers it.
//! - `copilot` (formula) - the AWS ECS tool, archived. GitHub Copilot is the
//!   *cask* `copilot-cli`.
//! - `antigravity` (npm) - not Google's; a placeholder by an unrelated
//!   maintainer. The cask `antigravity-cli` is the real one.
//! - `netcat` (formula) - GNU netcat 0.7.1, dormant since 2004.
//! - `telnet` (formula) - no bottle; `inetutils` supplies telnet.
//! - `markdownlint-cli2` - 907 stars.
//! - `cpanminus` - 782 stars.
//! - `dive` - last push December 2025.
//! - `pipenv` - removed; `uv` covers it.

use super::{Bundle, Pkg, Provider, Selector};
use crate::managers::ManagerId;
use crate::platform::Platforms;

// --- core -------------------------------------------------------------------

macro_rules! brew_pkg {
    ($name:literal) => {
        Pkg {
            name: $name,
            binary: Some($name),
            providers: &[Provider::new(ManagerId::Brew, $name)],
        }
    };
    ($name:literal, bin = $bin:literal) => {
        Pkg {
            name: $name,
            binary: Some($bin),
            providers: &[Provider::new(ManagerId::Brew, $name)],
        }
    };
    ($name:literal, formula = $formula:literal, bin = $bin:literal) => {
        Pkg {
            name: $name,
            binary: Some($bin),
            providers: &[Provider::new(ManagerId::Brew, $formula)],
        }
    };
}

/// A toolchain managed by mise. No `binary`: for a version-managed tool,
/// "some `go` is on PATH" is not "the `go` we asked for", so only mise's own
/// listing can satisfy it.
macro_rules! mise_pkg {
    ($name:literal, $spec:literal) => {
        Pkg {
            name: $name,
            binary: None,
            providers: &[Provider::new(ManagerId::Mise, $spec)],
        }
    };
}

macro_rules! npm_pkg {
    ($name:literal, pkg = $pkg:literal, bin = $bin:literal) => {
        Pkg {
            name: $name,
            binary: Some($bin),
            providers: &[Provider::new(ManagerId::Npm, $pkg)],
        }
    };
}

macro_rules! cask_pkg {
    ($name:literal) => {
        Pkg {
            name: $name,
            binary: None,
            providers: &[Provider::new(ManagerId::BrewCask, $name)],
        }
    };
    ($name:literal, bin = $bin:literal) => {
        Pkg {
            name: $name,
            binary: Some($bin),
            providers: &[Provider::new(ManagerId::BrewCask, $name)],
        }
    };
}

macro_rules! flatpak_pkg {
    ($name:literal, $id:literal) => {
        Pkg {
            name: $name,
            binary: None,
            providers: &[Provider::new(ManagerId::Flatpak, $id)],
        }
    };
}

static CORE: &[Pkg] = &[
    brew_pkg!("ripgrep", bin = "rg"),
    brew_pkg!("fd"),
    brew_pkg!("bat"),
    brew_pkg!("eza"),
    brew_pkg!("zoxide"),
    brew_pkg!("fzf"),
    brew_pkg!("jq"),
    brew_pkg!("yq"),
    brew_pkg!("sd"),
    brew_pkg!("git-delta", bin = "delta"),
    brew_pkg!("hyperfine"),
    brew_pkg!("tealdeer", bin = "tldr"),
    brew_pkg!("vim"),
    brew_pkg!("git"),
    brew_pkg!("gh"),
    brew_pkg!("chezmoi"),
    brew_pkg!("just"),
    brew_pkg!("mise"),
    brew_pkg!("direnv"),
    brew_pkg!("watchexec"),
    brew_pkg!("tokei"),
    brew_pkg!("lazygit"),
    brew_pkg!("difftastic", bin = "difft"),
    brew_pkg!("actionlint"),
    brew_pkg!("htop"),
    brew_pkg!("btop"),
    brew_pkg!("wget"),
    brew_pkg!("curl"),
    brew_pkg!("make"),
    brew_pkg!("man-db", bin = "man"),
    brew_pkg!("whois"),
    brew_pkg!("nmap"),
    brew_pkg!("typos", formula = "typos-cli", bin = "typos"),
    brew_pkg!("yamllint"),
    // GNU inetutils supplies telnet; the standalone formula has no bottle.
    brew_pkg!("telnet", formula = "inetutils", bin = "telnet"),
    // No maintained user-space provider (see the module notes). Distributions
    // ship a current one, so this resolves through the binary check on any
    // ordinary system and falls back to dnf where that is usable.
    Pkg {
        name: "netcat",
        binary: Some("nc"),
        providers: &[Provider::gated(
            ManagerId::Dnf,
            "netcat",
            Platforms::NOT_ATOMIC,
        )],
    },
    brew_pkg!("tmux"),
    brew_pkg!("devcontainer"),
    // No Homebrew formula: an OS-level container tool. Atomic images ship it,
    // so the binary check settles it there; elsewhere dnf can supply it.
    Pkg {
        name: "toolbox",
        binary: Some("toolbox"),
        providers: &[Provider::gated(
            ManagerId::Dnf,
            "toolbox",
            Platforms::NOT_ATOMIC,
        )],
    },
    Pkg {
        name: "powertmux",
        binary: Some("powertmux"),
        providers: &[Provider::tapped("powertmux", "powertmux/powertmux")],
    },
];

// --- shell ------------------------------------------------------------------

static SHELL: &[Pkg] = &[brew_pkg!("shellcheck"), brew_pkg!("shfmt")];

// --- prompt -----------------------------------------------------------------
// Exactly one of these is installed: the one `[shell] prompt` names.

static PROMPT: &[Pkg] = &[
    brew_pkg!("starship"),
    brew_pkg!("oh-my-posh"),
    // A bash library rather than a binary, so only the tap listing can
    // satisfy it.
    Pkg {
        name: "powerbash",
        binary: None,
        providers: &[Provider::tapped("powerbash", "powerbash/powerbash")],
    },
];

// --- security ---------------------------------------------------------------

static SECURITY: &[Pkg] = &[
    brew_pkg!("trivy"),
    brew_pkg!("gitleaks"),
    brew_pkg!("osv-scanner"),
    brew_pkg!("semgrep"),
    brew_pkg!("syft"),
    brew_pkg!("grype"),
    brew_pkg!("hadolint"),
];

// --- ai ---------------------------------------------------------------------

static AI: &[Pkg] = &[
    // The vendor script installs into ~/.local/bin, where it self-updates.
    // The binary check keeps npm from adding a second, staler copy.
    npm_pkg!(
        "claude-code",
        pkg = "@anthropic-ai/claude-code",
        bin = "claude"
    ),
    // The cask, not the formula: `copilot` the formula is AWS's ECS tool.
    cask_pkg!("copilot-cli", bin = "copilot"),
    npm_pkg!("codex", pkg = "@openai/codex", bin = "codex"),
    // The cask installs its binary as `agy`, not `antigravity`.
    cask_pkg!("antigravity-cli", bin = "agy"),
];

// --- languages --------------------------------------------------------------
// Each bundle is the toolchain plus its supporting tools, so a machine has
// every language ready without a second step.

static GO: &[Pkg] = &[
    mise_pkg!("go", "go@latest"),
    brew_pkg!("golangci-lint"),
    brew_pkg!("govulncheck"),
    brew_pkg!("gopls"),
    brew_pkg!("goreleaser"),
    brew_pkg!("delve", bin = "dlv"),
];

static RUST: &[Pkg] = &[
    mise_pkg!("rust", "rust@stable"),
    brew_pkg!("cargo-audit"),
    brew_pkg!("cargo-deny"),
    brew_pkg!("cargo-nextest"),
    brew_pkg!("cargo-binstall"),
    brew_pkg!("cargo-llvm-cov"),
    brew_pkg!("cargo-outdated"),
    brew_pkg!("bacon"),
    brew_pkg!("sccache"),
    brew_pkg!("taplo"),
    brew_pkg!("rust-analyzer"),
];

static PYTHON: &[Pkg] = &[
    mise_pkg!("python", "python@3.13"),
    brew_pkg!("uv"),
    brew_pkg!("ruff"),
    brew_pkg!("mypy"),
    brew_pkg!("pip-audit"),
    npm_pkg!("pyright", pkg = "pyright", bin = "pyright"),
];

static NODE: &[Pkg] = &[
    mise_pkg!("node", "node@lts"),
    mise_pkg!("bun", "bun@latest"),
    mise_pkg!("deno", "deno@latest"),
    brew_pkg!("pnpm"),
    brew_pkg!("biome"),
    brew_pkg!("oxlint"),
    brew_pkg!("typescript", bin = "tsc"),
    brew_pkg!("prettier"),
];

static JAVA: &[Pkg] = &[
    mise_pkg!("java", "java@corretto-21"),
    mise_pkg!("maven", "maven@latest"),
    mise_pkg!("gradle", "gradle@latest"),
    mise_pkg!("kotlin", "kotlin@latest"),
    mise_pkg!("ktlint", "ktlint@latest"),
];

static DOTNET: &[Pkg] = &[
    // The current LTS. `latest` would select a preview.
    mise_pkg!("dotnet", "dotnet@10"),
];

static RUBY: &[Pkg] = &[mise_pkg!("ruby", "ruby@latest")];

static ZIG: &[Pkg] = &[mise_pkg!("zig", "zig@latest"), brew_pkg!("zls")];

static PHP: &[Pkg] = &[
    // Bottled; mise's PHP builds from source.
    brew_pkg!("php"),
    brew_pkg!("composer"),
];

static LUA: &[Pkg] = &[
    brew_pkg!("lua"),
    brew_pkg!("luarocks"),
    brew_pkg!("stylua"),
    brew_pkg!("lua-language-server"),
];

static PERL: &[Pkg] = &[brew_pkg!("perl")];

static ELIXIR: &[Pkg] = &[
    // Both bottled; mise's Erlang compiles OTP from source.
    brew_pkg!("erlang", bin = "erl"),
    brew_pkg!("elixir"),
];

static POWERSHELL: &[Pkg] = &[brew_pkg!("powershell", bin = "pwsh")];

static ANDROID: &[Pkg] = &[
    // Google's unified `android` command-line tool (dl.google.com/android/cli):
    // `android init`, `android sdk`, `android emulator`, `android create`.
    // It installs SDK components itself, after the licences are accepted -
    // a deliberate act rather than an automated one.
    mise_pkg!("android-cli", "android-cli@latest"),
    brew_pkg!("scrcpy"),
    Pkg {
        name: "android-studio",
        binary: None,
        providers: &[Provider::gated(
            ManagerId::Flatpak,
            "com.google.AndroidStudio",
            Platforms::GRAPHICAL,
        )],
    },
];

// --- web, data, aws ---------------------------------------------------------

static WEB: &[Pkg] = &[
    brew_pkg!("stylelint"),
    brew_pkg!("htmlq"),
    brew_pkg!("pandoc"),
    npm_pkg!("pa11y", pkg = "pa11y", bin = "pa11y"),
];

static DATA: &[Pkg] = &[
    brew_pkg!("miller", bin = "mlr"),
    brew_pkg!("duckdb"),
    brew_pkg!("qsv"),
    brew_pkg!("sqlite", bin = "sqlite3"),
    brew_pkg!("sqlite-utils"),
];

static AWS: &[Pkg] = &[
    brew_pkg!("awscli", bin = "aws"),
    brew_pkg!("aws-sam-cli", bin = "sam"),
    brew_pkg!("cfn-lint"),
];

// --- desktop ----------------------------------------------------------------

static DESKTOP: &[Pkg] = &[
    // Flatpak applications expose no binary on PATH, so only the flatpak
    // listing can satisfy them.
    flatpak_pkg!("remmina", "org.remmina.Remmina"),
    flatpak_pkg!("spotify", "com.spotify.Client"),
    // Homebrew first, dnf only where Homebrew cannot serve.
    Pkg {
        name: "xdotool",
        binary: Some("xdotool"),
        providers: &[
            Provider::new(ManagerId::Brew, "xdotool"),
            Provider::gated(ManagerId::Dnf, "xdotool", Platforms::NOT_ATOMIC),
        ],
    },
];

// --- fonts ------------------------------------------------------------------
// Casks with no executable; only the cask listing can satisfy them.

static FONTS: &[Pkg] = &[
    cask_pkg!("font-0xproto-nerd-font"),
    cask_pkg!("font-blex-mono-nerd-font"),
    cask_pkg!("font-caskaydia-mono-nerd-font"),
    cask_pkg!("font-comic-shanns-mono-nerd-font"),
    cask_pkg!("font-droid-sans-mono-nerd-font"),
    cask_pkg!("font-fira-code-nerd-font"),
    cask_pkg!("font-go-mono-nerd-font"),
    cask_pkg!("font-jetbrains-mono-nerd-font"),
    cask_pkg!("font-sauce-code-pro-nerd-font"),
    cask_pkg!("font-source-code-pro"),
    cask_pkg!("font-ubuntu-nerd-font"),
];

/// A bundle on everywhere.
const fn bundle(name: &'static str, description: &'static str, packages: &'static [Pkg]) -> Bundle {
    Bundle {
        name,
        description,
        platforms: Platforms::ALL,
        selector: Selector::All,
        packages,
    }
}

/// Every bundle `nt` knows about, in the order they are reported.
pub static BUNDLES: &[Bundle] = &[
    bundle("core", "Terminal and git essentials", CORE),
    bundle("shell", "Shell script linting and formatting", SHELL),
    Bundle {
        name: "prompt",
        description: "The shell prompt named by [shell] prompt",
        platforms: Platforms::ALL,
        selector: Selector::Prompt,
        packages: PROMPT,
    },
    bundle(
        "security",
        "Vulnerability, secret and misconfiguration scanners",
        SECURITY,
    ),
    bundle("ai", "AI coding agents and assistants", AI),
    bundle(
        "go",
        "Go toolchain, linting, vulnerability scanning and debugging",
        GO,
    ),
    bundle(
        "rust",
        "Rust toolchain, auditing, coverage and language server",
        RUST,
    ),
    bundle("python", "Python, uv, linting, typing and auditing", PYTHON),
    bundle("node", "Node, Bun, Deno, pnpm and JavaScript tooling", NODE),
    bundle(
        "java",
        "Amazon Corretto, Maven, Gradle, Kotlin and ktlint",
        JAVA,
    ),
    bundle("dotnet", ".NET SDK", DOTNET),
    bundle("ruby", "Ruby", RUBY),
    bundle("zig", "Zig and its language server", ZIG),
    bundle("php", "PHP and Composer", PHP),
    bundle("lua", "Lua, LuaRocks, StyLua and its language server", LUA),
    bundle("perl", "Perl", PERL),
    bundle("elixir", "Erlang and Elixir", ELIXIR),
    bundle("powershell", "PowerShell", POWERSHELL),
    bundle(
        "android",
        "Android command-line tools, scrcpy and Android Studio",
        ANDROID,
    ),
    bundle("web", "HTML, CSS and accessibility tooling", WEB),
    bundle("data", "SQLite, CSV and columnar data tooling", DATA),
    bundle("aws", "AWS CLI, serverless and CloudFormation tooling", AWS),
    Bundle {
        name: "desktop",
        description: "Graphical applications and desktop helpers",
        platforms: Platforms::GRAPHICAL,
        selector: Selector::All,
        packages: DESKTOP,
    },
    Bundle {
        name: "fonts",
        description: "Nerd Fonts and programming typefaces",
        platforms: Platforms::GRAPHICAL,
        selector: Selector::All,
        packages: FONTS,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // These guard the catalog as data. They exist so a bad entry fails the
    // build rather than a user's machine.

    #[test]
    fn bundle_names_are_unique() {
        let mut seen = HashSet::new();
        for b in BUNDLES {
            assert!(seen.insert(b.name), "duplicate bundle name: {}", b.name);
        }
    }

    #[test]
    fn bundle_names_are_valid_cli_values() {
        for b in BUNDLES {
            assert!(
                !b.name.is_empty()
                    && b.name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    && !b.name.starts_with('-')
                    && !b.name.ends_with('-'),
                "bundle name is not usable on the command line: {:?}",
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
    fn provider_ids_are_unique_per_manager() {
        // Two packages naming the same formula would be installed once and
        // reported twice.
        let mut seen = HashSet::new();
        for b in BUNDLES {
            for p in b.packages {
                for pr in p.providers {
                    assert!(
                        seen.insert((pr.manager, pr.id)),
                        "{}/{} repeats provider {}:{}",
                        b.name,
                        p.name,
                        pr.manager,
                        pr.id
                    );
                }
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
    fn packages_without_a_binary_are_only_those_whose_listing_is_authoritative() {
        // Fonts and flatpak apps have no executable; mise toolchains are
        // version-managed, so a same-named binary on PATH proves nothing;
        // powerbash is a bash library. Anything else omitting `binary` would
        // be reinstalled whenever its manager happened not to report it.
        for b in BUNDLES {
            for p in b.packages.iter().filter(|p| p.binary.is_none()) {
                let ok = p.providers.iter().all(|pr| {
                    matches!(pr.manager, ManagerId::Flatpak | ManagerId::Mise)
                        || p.name.starts_with("font-")
                        || p.name == "powerbash"
                });
                assert!(ok, "{}/{} declares no binary", b.name, p.name);
            }
        }
    }

    #[test]
    fn mise_specs_are_tool_at_version() {
        for b in BUNDLES {
            for p in b.packages {
                for pr in p
                    .providers
                    .iter()
                    .filter(|pr| pr.manager == ManagerId::Mise)
                {
                    let (tool, version) = pr.id.split_once('@').unwrap_or_else(|| {
                        panic!(
                            "{}/{}: mise id {:?} is not tool@version",
                            b.name, p.name, pr.id
                        )
                    });
                    assert!(!tool.is_empty() && !version.is_empty(), "{}", pr.id);
                }
            }
        }
    }

    #[test]
    fn the_aws_formula_is_not_confused_with_the_github_copilot_cask() {
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

    #[test]
    fn the_prompt_bundle_offers_every_selectable_prompt() {
        let prompt = BUNDLES.iter().find(|b| b.name == "prompt").unwrap();
        let names: Vec<&str> = prompt.packages.iter().map(|p| p.name).collect();

        assert_eq!(prompt.selector, Selector::Prompt);
        for expected in crate::config::PROMPTS {
            assert!(
                names.contains(expected),
                "prompt {expected} is not in the bundle"
            );
        }
    }

    #[test]
    fn only_the_prompt_bundle_uses_the_prompt_selector() {
        for b in BUNDLES {
            assert_eq!(
                b.selector == Selector::Prompt,
                b.name == "prompt",
                "{}",
                b.name
            );
        }
    }

    #[test]
    fn graphical_bundles_are_the_ones_that_draw() {
        for b in BUNDLES {
            let graphical = b.platforms.needs_graphical;
            assert_eq!(
                graphical,
                matches!(b.name, "desktop" | "fonts"),
                "{} graphical={graphical}",
                b.name
            );
        }
    }

    #[test]
    fn every_requested_language_is_present() {
        // The brief: every language and its toolset, plus Corretto, Android
        // and PowerShell. A rename here should be deliberate.
        let names: HashSet<&str> = BUNDLES.iter().map(|b| b.name).collect();
        for expected in [
            "go",
            "rust",
            "python",
            "node",
            "java",
            "dotnet",
            "ruby",
            "zig",
            "php",
            "lua",
            "perl",
            "elixir",
            "powershell",
            "android",
        ] {
            assert!(names.contains(expected), "missing bundle {expected}");
        }
        let java = BUNDLES.iter().find(|b| b.name == "java").unwrap();
        assert!(
            java.packages
                .iter()
                .any(|p| p.providers[0].id.starts_with("java@corretto")),
            "Java must be Amazon Corretto"
        );
    }
}
