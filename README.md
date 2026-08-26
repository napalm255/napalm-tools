# napalm-tools

Fast, private, idempotent user-space system configuration for Linux. Binary: `nt`.

`nt` provisions a workstation entirely in user space. On an atomic Fedora
system (Bluefin, Silverblue) it never touches the immutable OS tree; on a
traditional Fedora install or under WSL it uses the same catalog and adapts
what it reaches for. Nothing is sent anywhere — there is no telemetry and no
network access beyond the package managers it drives.

## Quick start

```bash
cargo build --release
cp config.example.toml ~/.config/napalm-tools/config.toml

nt bundles              # what is in the catalog, and its state on this host
nt apply --dry-run      # what would change
nt apply                # make it so
```

## Commands

| Command | Does |
| --- | --- |
| `nt apply` | Converge this machine on the resolved configuration |
| `nt status` | Report desired versus installed state; changes nothing |
| `nt bundles` | List bundles and their effective state here |
| `nt config show` | Print the fully resolved configuration |
| `nt config path` | Print the configuration file path |
| `nt version` | Print the version alone, for scripts |
| `nt completions <shell>` | Generate completions for bash, zsh or fish |

`nt apply` takes `--dry-run`, `--upgrade`, `--strict` and `--no-dotfiles`, plus
a generated `--<bundle>` / `--no-<bundle>` pair for every bundle in the catalog.

## Output

Subprocess output is captured, not inherited. A run shows what step it is on
and how long each took, and the managers' several hundred lines of chatter stay
out of the way unless something in them matters.

```
  [1/1] brew install nmap inetutils ok (7.1s)

1 step in 7.1s

warnings:
  Warning: The following taps are not trusted:
    someone/tap
```

Two rules govern where things go:

- **stdout is the answer** - the rendered plan, the notes, any JSON, nothing
  else. `nt bundles --output json > file` produces a file that parses.
- **stderr is everything else** - progress, warnings, caveats, timings, errors.

`--output` selects `pretty`, `plain` or `json`; without it, `pretty` when
stderr is a terminal and `plain` otherwise, so pipes and CI logs stay readable.

| flag | subprocess output | logging |
| --- | --- | --- |
| *(none)* | captured, hidden behind a spinner | warnings |
| `-q` | captured, hidden, no progress | errors only |

`-q` is silent on success - no progress, no summary, and no answer either,
including from query commands like `nt version`. Silence means it worked;
a failure still reports. Asking for quiet and for output at once is
contradictory, and quiet is the more specific request.
| `-v` | raw passthrough, spinner off | info |
| `-vv` | raw passthrough | debug |

Homebrew's `==> Caveats` blocks and deprecation warnings are collected while
scrolling past and shown once at the end, where they can be read.

### Machine-readable

`nt` holds its catalog to a standard - machine-readable output, non-interactive,
meaningful exit codes - so it meets it too.

```bash
nt bundles --output json | jq -r '.bundles[] | select(.enabled) | .name'
nt apply --dry-run --output json | jq -r '.actions[].command'
nt apply --dry-run --output json | jq '.unavailable[] | {package, reason}'
```

## How it decides

Configuration is layered, lowest to highest:

```
catalog defaults -> [bundles] etc. -> matching [host."glob"] tables, in file order -> CLI flags
```

Host tables are applied **in the order they appear in the file**, so later
entries win. Write them general first, specific last. There is no hidden
specificity ranking.

Each package declares an ordered list of providers, and `nt` takes the first
one that is both permitted on this platform and backed by an available manager.
That is how "Homebrew first, dnf as a last resort" is expressed — as data,
per package, rather than as a global rule.

If no provider applies, the package is reported as unavailable and skipped.
It is never installed by some other route.

### Why `dnf` is gated on the platform, not on `PATH`

On Bluefin, `dnf` is on `PATH` and appears to work. Anything it installs is
discarded at the next OS update. `nt` therefore refuses `dnf` on any
ostree-booted host regardless of whether the binary exists:

```
$ nt apply --dry-run --desktop        # on Bluefin
Unavailable:
  ! xdotool (desktop): no user-space provider on an atomic host
```

The same catalog on a traditional Fedora host plans `dnf install -y xdotool`.

### Flatpak scope

Installs go to the user scope, so `nt` never mutates system state. The
installed-check consults **both** scopes, because on a typical desktop the
existing applications were installed system-wide — treating those as missing
would mean reinstalling every one of them as a user copy.

### Already present counts, whatever installed it

A package may declare the executable it provides. If that binary resolves on
`PATH`, the package is satisfied regardless of which manager - if any - put it
there. This matters more than it sounds:

- An atomic base image already ships `jq`, `git`, `vim`, `fzf`, `tmux` and
  more. Without this rule `nt` installs brew copies that shadow them.
- Claude Code installs itself into `~/.local/bin` and self-updates there.
  Without this rule `nt` adds a second, staler copy via npm.
- Formula names often differ from binary names: `ripgrep`/`rg`,
  `git-delta`/`delta`, `tealdeer`/`tldr`, `miller`/`mlr`.

On the development machine this prevents eight redundant installs on a default
run. A manager that genuinely owns the package still takes precedence, so
`--upgrade` keeps working.

## Managers

`brew`, `brew-cask`, `npm`, `bun`, `flatpak`, and `dnf` (traditional Fedora
only). Each is queried in a single bulk call, so a converged machine re-runs in
about the time it takes those managers to start.

Formulae and casks are separate managers rather than one with a flag, because
their namespaces genuinely collide: the formula `copilot` is the AWS ECS tool
while the cask `copilot-cli` is GitHub Copilot.

## Bundles

Seventeen bundles, ninety-nine packages. `core`, `shell`, `security` and `ai` are on
by default; everything else is opt-in.

Language tooling is one bundle per language, so a machine that never touches Go
skips that tooling entirely. Language *runtimes* are separate opt-in bundles,
because `mise` manages runtimes where it is in use and two things managing one
runtime means `PATH` order decides the winner.

Every third-party package is checked against the project's dependency rules -
1000+ stars, a commit within six months, not archived, compatible licence -
with a carve-out for first-party tooling such as `govulncheck`. Packages
considered and rejected are recorded in the design notes so they are not
reintroduced.

Tool selection favours machine-readable output (JSON or SARIF), non-interactive
operation, precise `file:line:col` and single static binaries - which is why
`ruff`, `biome`, `oxlint` and `uv` appear rather than their predecessors.

## Configuration

See [`config.example.toml`](config.example.toml) for a documented example.

Environment overrides, mainly for testing: `NT_CONFIG`, `NT_HOSTNAME`,
`NT_OS_RELEASE`, `NT_OSTREE_MARKER`.

## Development

```bash
just            # list recipes
just test       # unit and end-to-end tests
just lint       # fmt --check + clippy -D warnings
just security   # osv-scanner, gitleaks, trivy
just ci         # all of the above
```

The decision engine is a pure function — `plan::build` takes resolved
configuration, a platform, and a snapshot of installed packages, and returns a
list of actions. `--dry-run` renders that list and a real run executes it, so
the two cannot drift apart. It also means the interesting behaviour is testable
without spawning a single subprocess.

## Scope

This is the foundation: preferences, platform detection, package provisioning
and the chezmoi bootstrap. Still to come, each as its own design: a compiled-in
script system replacing `~/bin`, `nt --shell-init` replacing `.bashrc.d/`, and
a terminal interface under the reserved bare `nt config`.

Design notes live in [`docs/superpowers/specs/`](docs/superpowers/specs/).

## Licence

MIT.
