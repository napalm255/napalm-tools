# AGENTS.md

Guidance for any agent - or person - working in this repository. Keep this
file and `README.md` current with every change that affects them; a change
that alters a command, a flag, a bundle, a manager, or a decision recorded
here is not finished until both files say so.

## What this is

`napalm-tools` builds one binary, `nt`, that provisions a Linux workstation in
user space and is opinionated about what a workstation should have: every
language toolchain and its supporting tools, the security scanners, the AI
agents, a chosen shell prompt. It targets Fedora Workstation, Fedora Server,
the official Fedora container image, Fedora under WSL, and Bluefin (Fedora
atomic). It never touches an immutable OS tree and never runs as root.

## Layout

| Path | Purpose |
| --- | --- |
| `src/bundles/catalog.rs` | **The catalog.** Data only. Every package, its providers in preference order, and its binary |
| `src/bundles/mod.rs` | `Bundle`, `Pkg`, `Provider`, `Selector` types |
| `src/managers/` | One module per package manager: `brew`, `brew_cask`, `npm`, `bun`, `flatpak`, `mise`, `dnf`. `mod.rs` holds `Cmd` (subprocess handling) and the `Manager` trait |
| `src/platform.rs` | Detection: `fedora_family`, `atomic`, `wsl`, `container`, `graphical` |
| `src/config/` | `config.toml` types, hostname globs, layering into `Resolved`. Validation of user input lives in `merge.rs` |
| `src/plan.rs` | **Pure.** Snapshot + config + platform -> `ActionPlan`. Also the bootstrap decision |
| `src/execute.rs` | Takes the snapshot, runs commands |
| `src/privilege.rs` | Decides up front whether a run needs `sudo`; asks once |
| `src/dotfiles.rs` | chezmoi bootstrap |
| `src/shell.rs` | `nt shell-init` |
| `src/report.rs`, `src/ui/` | Text and JSON rendering, theme, spinner, output scanning |
| `src/cli.rs` | clap command tree. Flags are declared per command, never globally |
| `src/main.rs` | Dispatch and the three phases of `apply`: bootstrap, snapshot, converge |
| `tests/cli.rs` | Integration tests driving the binary with `NT_*` overrides |
| `tests/e2e/` | Container-driven end-to-end runs (Fedora and Bluefin) |
| `.devcontainer/` | The development image; also the Fedora e2e image |
| `docs/superpowers/specs/` | Design records. Read the latest before changing behaviour |

## Commands

```bash
mise install          # toolchain (rust, just, cargo-binstall) from mise.toml
just setup            # cargo-deny, cargo-audit
just lint             # fmt --check, clippy -D warnings
just test             # unit + integration
just security         # cargo audit, cargo deny, plus osv-scanner/gitleaks/trivy if present
just ci               # lint test security
just e2e-fedora       # real `nt apply` in the devcontainer image (needs podman)
just e2e-bluefin      # same in a Bluefin image
just audit-binaries   # catalog binary names vs this machine
```

Every change must pass `just ci`. A change to the catalog or a manager should
also pass `just e2e-fedora`.

## Invariants - do not break these

1. **`plan::build` is pure.** No I/O, no subprocesses. `--dry-run` renders its
   output and a real run executes it; they cannot diverge.
2. **`dnf` is gated on the platform, never on `PATH`.** It is on `PATH` under
   Bluefin and appears to work; anything it installs vanishes at the next OS
   update.
3. **Flatpak installs go to `--user`; the installed check reads both scopes,
   `--app` only.** The user scope starts with no remotes, so the plan adds
   Flathub first.
4. **mise packages have no `binary`.** A version-managed toolchain is
   satisfied only by mise's own global config. Their ids are `tool@version`.
5. **Stdout is the answer, stderr is everything else.** `--output json` must
   produce a document that parses. Stdout decoration follows stdout being a
   terminal, not stderr.
6. **Every flag is declared only where it works.** `nt version -q` is an
   error. Bundle and prompt names are validated by clap.
7. **User-supplied names are validated at the boundary** (`config/merge.rs`):
   no flag-shaped `[extra]` entries, no unknown bundles or managers.
8. **`apply` refuses root.**
9. **JSON keys are an interface.** Tests pin them. Add keys; do not rename.
10. **No `unwrap`/`expect` on input paths.** `expect` only where clap or a
    catalog invariant makes failure impossible, with the reason in the message.

## Adding a package

1. Vet it against the rules below and record the result in the table.
2. Add it to the right bundle in `catalog.rs` using the `brew_pkg!`,
   `mise_pkg!`, `npm_pkg!`, `cask_pkg!` or `flatpak_pkg!` macro. Declare the
   binary it installs if it installs one; get the name right - a wrong binary
   is silent (see `scripts/audit-binaries.py`).
3. Prefer Homebrew for command-line tools and mise for toolchains and
   anything that wants a JDK. Use `dnf` only as a gated last resort.
4. `just ci`, then `just e2e-fedora`, then update `README.md`.

## Dependency rules

Third-party packages need **1000+ GitHub stars, a push within six months, not
archived, a compatible licence**. Tooling published by a language or platform
owner is judged on official status instead (the first-party carve-out).
Rejected packages are listed at the top of `catalog.rs` so they are not
reintroduced.

### Vetting record (2026-08-25)

Stars and last push as of that date. Everything listed passes unless noted.

| Package | Repo | Stars | Pushed | Note |
| --- | --- | ---: | --- | --- |
| ripgrep | BurntSushi/ripgrep | 67.6k | 2026-08 | |
| fd, bat, hyperfine | sharkdp/* | 44k, 60k, 29k | 2026-08, 2026-08, 2026-04 | |
| eza | eza-community/eza | 23k | 2026-08 | |
| zoxide | ajeetdsouza/zoxide | 39k | 2026-08 | |
| fzf | junegunn/fzf | 83k | 2026-08 | |
| jq, yq | jqlang/jq, mikefarah/yq | 35k, 16k | 2026-08 | |
| sd | chmln/sd | 7.3k | 2026-02 | Six months exactly; watch |
| git-delta | dandavison/delta | 32k | 2026-08 | |
| tealdeer | tealdeer-rs/tealdeer | 6.5k | 2026-08 | |
| vim, git, gh | - | 41k, 63k, 46k | 2026-08 | |
| chezmoi | twpayne/chezmoi | 21k | 2026-08 | |
| just | casey/just | 35k | 2026-08 | |
| mise | jdx/mise | 33k | 2026-08 | |
| direnv | direnv/direnv | 15k | 2026-03 | |
| watchexec | watchexec/watchexec | 7.1k | 2026-08 | |
| tokei | XAMPPRocky/tokei | 15k | 2026-05 | |
| lazygit | jesseduffield/lazygit | 82k | 2026-08 | |
| difftastic | Wilfred/difftastic | 26k | 2026-08 | |
| actionlint | rhysd/actionlint | 4.2k | 2026-07 | |
| htop, btop | - | 8.3k, 34k | 2026-08 | |
| nmap, tmux | - | 13k, 49k | 2026-08 | |
| typos | crate-ci/typos | 4.1k | 2026-08 | |
| yamllint | adrienverge/yamllint | 3.4k | 2026-08 | |
| devcontainer | devcontainers/cli | 2.9k | 2026-08 | |
| toolbox | containers/toolbox | 3.5k | 2026-08 | OS-native; dnf fallback |
| powertmux, powerbash | user's own taps | - | - | Not subject to the rules |
| shellcheck, shfmt | koalaman/shellcheck, mvdan/sh | 40k, 9k | 2026-08 | |
| starship | starship/starship | 60k | 2026-08 | |
| oh-my-posh | JanDeDobbeleer/oh-my-posh | 23k | 2026-08 | |
| trivy, gitleaks, osv-scanner | - | 38k, 29k, 11k | 2026-08 | |
| semgrep, syft, grype, hadolint | - | 16k, 9.5k, 13k, 12k | 2026-08 | |
| claude-code, copilot-cli, codex | - | 143k, 11k, 118k | 2026-08 | antigravity-cli: Google's cask |
| golangci-lint, gopls, goreleaser, delve | - | 19k, 8k, 16k, 25k | 2026-08 | |
| govulncheck | golang/vuln | 510 | 2026-08 | **Carve-out**: the Go team's scanner |
| cargo-audit, cargo-deny, cargo-nextest | - | 1.9k, 2.4k, 3.2k | 2026-08 | |
| cargo-binstall, cargo-llvm-cov, cargo-outdated | - | 2.8k, 1.5k, 1.4k | 2026-08, 2026-08, 2026-06 | |
| bacon, sccache, taplo, rust-analyzer | - | 3.4k, 7.6k, 2.4k, 17k | 2026-08 | |
| ruff, uv, mypy, pip-audit, pyright | - | 49k, 89k, 21k, 1.4k, 16k | 2026-08 | |
| biome, oxlint, typescript, prettier, pnpm | - | 26k, 22k, 111k, 52k, 36k | 2026-08 | |
| bun, deno | oven-sh/bun, denoland/deno | 96k, 108k | 2026-08 | |
| Corretto | corretto/corretto-21 | 121 | 2026-08 | **Carve-out**: Amazon's JDK, requested by name |
| kotlin, gradle, maven, ktlint | - | 53k, 19k, 5.3k, 6.7k | 2026-08 | Via mise, against Corretto |
| dotnet | dotnet/sdk | 3.2k | 2026-08 | `dotnet@10`, the LTS |
| ruby | ruby/ruby | 24k | 2026-08 | |
| zig, zls | ziglang/zig, zigtools/zls | 43k, 5.1k | 2025-11, 2026-08 | Zig's GitHub is a mirror; development moved to Codeberg and is active |
| php, composer | php/php-src, composer/composer | 40k, 30k | 2026-08 | |
| lua, luarocks, stylua, lua-language-server | - | 10k, 3.7k, 2.3k, 4.3k | 2026-08 | |
| perl | Perl/perl5 | 2.3k | 2026-08 | |
| erlang, elixir | erlang/otp, elixir-lang/elixir | 12k, 27k | 2026-08 | |
| powershell | PowerShell/PowerShell | 55k | 2026-08 | Homebrew core formula, Linux bottle |
| android-cli | Google (dl.google.com/android/cli) | - | - | First-party: the unified `android` CLI. Via mise |
| scrcpy | Genymobile/scrcpy | 148k | 2026-08 | |
| Android Studio | Google, Flathub | - | - | First-party. Flatpak, desktop only |
| stylelint, htmlq, pandoc, pa11y | - | 12k, 7.6k, 46k, 4.5k | 2026-08, 2026-05, 2026-08, 2026-08 | |
| miller, duckdb, qsv, sqlite-utils | - | 10k, 41k, 3.8k, 2.2k | 2026-08 | |
| awscli, aws-sam-cli, cfn-lint | - | 17k, 6.7k, 2.6k | 2026-08 | |
| remmina | FreeRDP/Remmina | 2.5k | 2026-02 | GitHub is a mirror; GitLab upstream is active |
| xdotool | jordansissel/xdotool | 3.8k | 2026-06 | |
| Nerd Fonts | ryanoasis/nerd-fonts | 64k | 2026-08 | |

Rejected: `markdownlint-cli2` (907 stars), `cpanminus` (782), `dive` (last
push 2025-12), `pipenv` (replaced by `uv`), plus the older rejections listed
in `catalog.rs`.

## Environment overrides

For tests and for exercising another platform's code path on this machine:
`NT_CONFIG`, `NT_HOSTNAME`, `NT_OS_RELEASE`, `NT_OSTREE_MARKER`,
`NT_CONTAINER_MARKER`, `NT_SESSION_DIR`, `NT_TOOL_DIRS`, `NT_FAKE_UID`.

## Style

Conventional Commits, imperative subject, no AI attribution trailers. Tests
are named as sentences describing behaviour. Comments explain why, not what.
