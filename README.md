# napalm-tools

Fast, private, idempotent user-space system configuration for Linux. Binary:
`nt`.

`nt` is an opinionated setup tool. Run it on a fresh machine and you get every
language toolchain and its supporting tools, the security scanners, the AI
agents, a shell prompt, and your dotfiles - in user space, from Homebrew and
mise, with `dnf` only as a gated last resort. Run it again and it does
nothing, quickly.

It targets Fedora Workstation, Fedora Server, the official Fedora container
image, Fedora under WSL, and Bluefin (Fedora atomic). On an atomic host it
never touches the immutable OS tree. Nothing is sent anywhere: no telemetry,
and no network access beyond the package managers it drives.

## Quick start

```bash
cargo build --release
./target/release/nt apply --dry-run   # what would change, including bootstrap
./target/release/nt apply             # make it so
./target/release/nt status            # where things stand
```

No configuration file is needed. To turn something off, or pick a different
prompt, copy [`config.example.toml`](config.example.toml) to
`~/.config/napalm-tools/config.toml`.

## Commands

| Command | Does |
| --- | --- |
| `nt apply` | Bootstrap the managers if needed, then converge on the configuration |
| `nt status` | Desired versus installed, per bundle; `--detail` for every package |
| `nt bundles` | The catalog and each bundle's state here; `--detail` for packages and providers |
| `nt config show` | The fully resolved configuration |
| `nt config path` | The configuration file path |
| `nt shell-init <shell>` | The line that activates the configured prompt |
| `nt version` | The version alone, for scripts |
| `nt completions <shell>` | Completions for bash, zsh or fish |

Flags appear only where they do something:

| Flag | Commands |
| --- | --- |
| `--config`, `--output pretty\|plain\|json`, `-v` | apply, status, bundles, config show |
| `--skip <bundle>`, `--only <bundle>` (repeatable) | apply, status, bundles, config show |
| `--detail` | status, bundles |
| `--dry-run`, `--upgrade`, `--strict`, `--no-dotfiles`, `--prompt`, `-q` | apply |

`nt version -q` is an error, not a no-op. Bundle and prompt names are
validated as they are parsed, so completions offer them and a typo fails.

## What `apply` does

Three phases, and `--dry-run` shows all three.

1. **Bootstrap.** If Homebrew is missing it is installed - its prerequisites
   from `dnf` where dnf is usable, then the official installer. If mise is
   missing it comes from Homebrew. On Bluefin both are already there, so this
   phase is empty.
2. **Snapshot.** One bulk query per manager: what is installed, which taps
   exist and are trusted, which flatpak remotes the user scope has, which
   catalog binaries are on `PATH`. A converged machine re-runs in the time
   it takes those managers to start.
3. **Converge.** One install command per manager, preceded by whatever it
   needs: the Flathub remote in the user scope, a tap, trusting the tap.
   Then the dotfiles step.

`apply` refuses to run as root. Homebrew refuses too, and a root-owned
`~/.local` is a trap for every tool that comes after.

```
$ nt apply --dry-run
Dry run - no changes will be made.
🧰 Bootstrap:
  + sudo dnf install -y procps-ng curl file git gcc
  + bash -c 'curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh | NONINTERACTIVE=1 bash'
  + brew install mise

⬇️  Steps (61 packages to install):
  + flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  + brew tap powertmux/powertmux
  + brew trust --tap powertmux/powertmux
  + brew install ripgrep fd bat ...
  + mise use --global --yes go@latest rust@stable python@3.13 node@lts ...
```

## Bundles

Every bundle is on. The only things that turn one off are the platform, a
`[bundles] name = false` line, or `--skip` / `--only` for a run.

| Bundle | Contents |
| --- | --- |
| `core` | Terminal and git essentials: ripgrep, fd, bat, eza, zoxide, fzf, jq, yq, delta, gh, chezmoi, just, mise, tmux, nmap, ... |
| `shell` | shellcheck, shfmt |
| `prompt` | The one prompt named by `[shell] prompt`: starship (default), oh-my-posh or powerbash |
| `security` | trivy, gitleaks, osv-scanner, semgrep, syft, grype, hadolint |
| `ai` | Claude Code, GitHub Copilot CLI, Codex, Antigravity |
| `go` | go (mise), golangci-lint, govulncheck, gopls, goreleaser, delve |
| `rust` | rust stable (mise), cargo-audit, cargo-deny, cargo-nextest, cargo-binstall, cargo-llvm-cov, cargo-outdated, bacon, sccache, taplo, rust-analyzer |
| `python` | python 3.13 (mise), uv, ruff, mypy, pip-audit, pyright |
| `node` | node LTS, bun, deno (mise), pnpm, biome, oxlint, typescript, prettier |
| `java` | Amazon Corretto 21, Maven, Gradle, Kotlin, ktlint (all mise) |
| `dotnet` | .NET 10 SDK (mise) |
| `ruby` | ruby (mise) |
| `zig` | zig (mise), zls |
| `php` | php, composer |
| `lua` | lua, luarocks, stylua, lua-language-server |
| `perl` | perl |
| `elixir` | erlang, elixir |
| `powershell` | pwsh |
| `android` | Google's `android` CLI (mise), scrcpy, Android Studio (flatpak, desktop only) |
| `web` | stylelint, htmlq, pandoc, playwright and its Chromium |
| `data` | miller, duckdb, qsv, sqlite, sqlite-utils |
| `aws` | awscli, aws-sam-cli, cfn-lint |
| `desktop` | Flatpak applications; needs a desktop session |
| `fonts` | Nerd Fonts; needs a desktop session |

`nt bundles --detail` lists every package with its providers.

**Homebrew** supplies command-line tools: bottled for Linux, no sudo. **mise**
supplies language toolchains and anything that wants a JDK, pinned per user
in its global config; the Kotlin, Gradle and Maven formulae would each pull
Homebrew's own OpenJDK in beside Corretto, so they come through mise against
the Corretto it installs. A mise package is satisfied only by mise's own
listing - "some `go` on PATH" is not "the `go` we asked for". For everything
else, an executable already on `PATH` counts, whatever put it there: the OS
image, a vendor script, another manager. That is what keeps `nt` from
installing a second `jq` beside the one Bluefin ships, or a second Claude
Code beside the self-updating one.

Browsers: a system Chromium is not available everywhere `nt` runs (no
`dnf` on atomic hosts; the Flatpak is sandboxed and desktop-only), so the
`web` bundle installs Playwright from npm and its own Chromium with
`playwright install chromium` - user-space on every platform, and planned
only when no complete revision is present.

The `android` bundle installs Google's unified `android` command-line tool,
which manages the SDK, emulators and projects. SDK components themselves are
not installed, because that requires accepting licences and should be a
deliberate act: `android init`, then `android sdk install`.

Every third-party package is vetted against the project's dependency rules -
1000+ stars, a push within six months, not archived, compatible licence -
with a carve-out for first-party tooling (`govulncheck`, Corretto). The full
record, and what was rejected, is in [`AGENTS.md`](AGENTS.md).

## Shell prompt

```toml
[shell]
prompt = "starship"   # or "oh-my-posh" or "powerbash"
```

Only the chosen prompt is installed. In your shell's start-up file:

```bash
eval "$(nt shell-init bash)"
```

Changing the prompt is then a configuration change, not an edit to three
files. `powerbash` is bash-only and `shell-init` says so for any other shell.

## Platforms

`nt` detects five facts and everything follows from them:

| Fact | From | Effect |
| --- | --- | --- |
| `fedora_family` | `/etc/os-release` `ID` or `ID_LIKE` | `dnf` is a possible last resort |
| `atomic` | `/run/ostree-booted` exists | `dnf` is refused even though it is on `PATH`; bootstrap skips its prerequisites |
| `wsl` | `$WSL_DISTRO_NAME` or `microsoft` in the kernel version | Never graphical |
| `container` | `/run/.containerenv` or `/.dockerenv` | Never graphical |
| `graphical` | `/usr/share/wayland-sessions` or `/usr/share/xsessions` has entries | `desktop`, `fonts`, Android Studio, flatpak at all |

`graphical` is judged by files on disk rather than `$DISPLAY`, which is unset
in exactly the shells - SSH, agents, cron - that most often run a setup tool.

### Why `dnf` is gated on the platform, not on `PATH`

On Bluefin, `dnf` is on `PATH` and appears to work. Anything it installs is
discarded at the next OS update. `nt` therefore refuses `dnf` on any
ostree-booted host regardless of whether the binary exists.

### Flatpak

Installs go to the user scope, so `nt` never mutates system state, and the
user scope starts with no remotes, so `nt` adds Flathub to it first. The
installed check consults both scopes and only applications - on a typical
desktop the existing applications were installed system-wide, and the
runtimes outnumber them.

## Output

**stdout is the answer** - the rendered plan, the status, any JSON. **stderr
is everything else** - progress, warnings, timings, errors. `nt bundles
--output json > file` produces a file that parses, and `nt bundles > file`
never contains an escape sequence, however lively the terminal.

`--output` selects `pretty`, `plain` or `json`; without it, `pretty` when
stdout is a terminal and `plain` otherwise. Pretty output uses colour, and
emoji where the terminal is UTF-8.

Subprocess output is captured and shown behind a spinner; `-v` streams it
through untouched and hands the terminal to the command, which is also how
to answer a prompt one insists on. Homebrew's `==> Caveats` blocks and
deprecation warnings are collected and shown once at the end.

A failed step does not stop the run: package steps are independent, so the
rest still run and every failure is listed at the end with its output. Only
a failed bootstrap ends the run, and the dotfiles step is skipped if any
package step failed, since its scripts may assume the packages exist. The
exit code is 1 when anything failed.

Steps that may need privileges - `dnf`, Homebrew's installer, a chezmoi run
script that mentions `sudo` - are known in advance, so `nt` asks for the
password once, before anything runs and before the spinner starts.

```bash
nt apply --dry-run --output json | jq -r '.actions[] | select(.privileged) | .command'
nt status --output json | jq '.totals'
nt status --output json | jq -r '.packages[] | select(.state == "missing") | .name'
nt bundles --output json | jq -r '.bundles[] | select(.applicable | not) | "\(.name): \(.reason)"'
```

## Configuration

See [`config.example.toml`](config.example.toml). Layers, lowest to highest:

```
catalog (everything on) -> [bundles] etc. -> matching [host."glob"] tables, in file order -> flags
```

Host tables apply in the order they appear in the file, so later entries win.
`[extra]` names packages outside the catalog per manager (including `mise`
as `tool@version`); they are validated so `"--force"` cannot become a flag
on an install command.

Environment overrides, for tests and for exercising another platform's code
path: `NT_CONFIG`, `NT_HOSTNAME`, `NT_OS_RELEASE`, `NT_OSTREE_MARKER`,
`NT_CONTAINER_MARKER`, `NT_SESSION_DIR`, `NT_TOOL_DIRS`. `NT_FAKE_UID=0` makes
`apply` behave as if run by root, to test the refusal; it can only force root,
never hide it.

## Development

```bash
mise install      # toolchain from mise.toml
just setup        # cargo-deny, cargo-audit
just ci           # lint, test, security
just e2e-fedora   # a real `nt apply` inside the devcontainer image
just e2e-bluefin  # the same inside a Bluefin image
just clean        # remove everything the repo created: target/, completions/, e2e images
```

Rust is pinned by `rust-toolchain.toml` and managed by rustup; `just` and
`cargo-binstall` come from `mise.toml`. `just setup` installs all of it,
and the justfile puts `~/.cargo/bin` and mise's shims on `PATH`, so recipes
work whether or not your shell has activated either.

The repository ships a devcontainer (`.devcontainer/`) built from the official
Fedora image, pinned by digest, with an ordinary user, passwordless sudo and
mise. The unit and integration tests run inside it; the same image is what
`just e2e-fedora` runs `nt apply` in, so the tests and the target are one
thing. `just e2e-bluefin` does the same in a Bluefin image with the atomic
marker set. CI runs lint, tests, cargo-deny, cargo-audit, and both end-to-end
jobs.

The decision engine is a pure function: `plan::build` takes resolved
configuration, a platform and a snapshot and returns actions. `--dry-run`
renders that list and a real run executes it, so the two cannot drift apart,
and the interesting behaviour is testable without spawning a subprocess.

[`AGENTS.md`](AGENTS.md) describes the layout, the invariants, and how to add
a package. Design records are in
[`docs/superpowers/specs/`](docs/superpowers/specs/).

## Licence

MIT.
