# napalm-tools foundation (`nt` v0.1) - design

*Status: implemented. Written 2026-08-25, before implementation; kept as the
record of what was decided and why. Where the implementation diverged, the
divergences are listed at the end.*

## Context

`napalm-tools` is a greenfield repo — currently only a `LICENSE`. The goal is a
fast, private, idempotent system-configuration tool in Rust, binary `nt`, that
provisions a Linux workstation entirely in **user space**: no touching an
immutable OS tree on Fedora atomic distros (Bluefin/Silverblue), while still
working on standard Fedora and WSL.

The full vision spans six subsystems (preferences, platform detection, package
provisioning, a compiled-in script/plugin system, `nt --shell-init` shell
integration, and a ratatui TUI with an animated fire effect). Speccing all six
at once yields a wishlist, not something executable. **This plan covers the
foundation only** — preferences, platform detection, package provisioning,
chezmoi bootstrap, and the CLI skeleton. That ships a genuinely useful `nt`.
Scripts/shell-init/TUI get their own brainstorm cycles afterward, designed
against interfaces that by then actually exist.

Intended outcome: on a fresh machine, `nt apply` converges user-space packages
and pulls down dotfiles in one command, and a routine re-run is a near-instant
no-op.

### Findings from environment probing (these shaped the design)

- `/run/ostree-booted` exists on this Bluefin box — the canonical atomic test.
  Use it rather than sniffing `ID=bluefin`.
- **`dnf` is on `PATH` on Bluefin.** A naive `which dnf` availability check
  would "install" packages that silently vanish on the next OS update. `dnf`
  must be gated on `!atomic`, not on `PATH`. This is the single most important
  correctness constraint in the design.
- `/etc/os-release` here: `ID=bluefin`, `ID_LIKE="fedora"`, `VARIANT_ID=bluefin-dx-nvidia-open`.
  Detect the Fedora family via `ID_LIKE`, not `ID`.
- `toml` 1.1.4 has a `preserve_order` feature (indexmap-backed) — required for
  file-ordered `[host."..."]` tables.
- Hostname is the FQDN (`napalm-desktop.local.naponline.net`) and is readable
  from `/proc/sys/kernel/hostname` — no dependency needed.

Inventorying the machine's real package set then surfaced three further
requirements the framework would otherwise have missed. This is why the seed
catalog is specced alongside the framework rather than after it — the catalog
is the framework's requirements document.

- **`flatpak list --user` is empty here.** All 106 flatpaks (Discord, Chrome,
  Steam, Postman…) are `--system` installs. A `--user`-only installed-check
  would report every one of them missing and try to reinstall them all. The
  installed-check must query **both scopes**; installs separately prefer
  `--user`.
- **Two brew leaves come from third-party taps** —
  `openclaw/tap/goplaces` and `powertmux/powertmux/powertmux`. `Provider`
  needs an optional `tap`, and `nt` must `brew tap` before installing. Not a
  corner case: the user maintains `homebrew-powerbash` and
  `homebrew-powertmux`.
- **`brew list --formula` returns 189 entries but only 43 are leaves** — the
  rest are transitive dependencies. For the idempotency set-diff the full list
  is correct and cheaper (installed is installed). For `nt status` reporting,
  use `brew leaves` so output reflects what was actually asked for.

Also noted, and deliberately *not* acted on: `pipx` and `uv` are both present,
but `hass-cli` came from the brew `homeassistant-cli` formula, so no Python
tool manager is needed for v0.1. `mise` is installed but is a version manager,
not a package manager. `bun` is absent entirely and will simply report
unavailable. One package (`zoom`) is rpm-ostree layered; layering stays out of
scope.

### Decisions already made (do not relitigate)

| Decision | Choice |
|---|---|
| Scope | Foundation only |
| Package model | Compiled-in bundle catalog + toggles, plus an `[extra]` escape hatch |
| Host overrides | Single file, ordered `[host."glob"]` tables, later-in-file wins |
| Atomic fallback | Skip with a clear warning; add `flatpak --user` as a first-class manager |
| Upgrades | Install missing by default; upgrade only on `--upgrade` |
| Dotfiles | chezmoi bootstrap as a post-package step inside `nt apply` |
| CLI parity | `--x` / `--no-x` flags generated at runtime by iterating `BUNDLES` |
| Catalog depth | Full bundle taxonomy + a ~12-package adversarial seed that exercises every framework path; remaining ~35 entries bulk-filled after as pure data |
| GUI flatpaks | Managed, but via an opt-in `desktop` bundle that is off by default and gated off on WSL |

## Architecture

The core is a **pure function**:
`(ResolvedConfig, Platform, installed-snapshot) -> ActionPlan`.

```
config.toml ─┐
CLI flags   ─┼─► resolve() ─► ResolvedConfig ─┐
defaults    ─┘                                ├─► plan() ─► ActionPlan ─┬─► render()   (--dry-run)
                          platform::detect() ─┤                         └─► execute()  (real run)
                          managers::snapshot()┘
```

`--dry-run` and a real run share one code path — dry-run renders the plan,
execute runs it. They cannot diverge. And because `plan()` touches nothing, the
whole decision engine is unit-testable with zero subprocesses.

## Files to create

```
Cargo.toml                     [[bin]] name = "nt"
justfile                       setup fmt lint test security build run clean
src/main.rs                    entry, error reporting, exit codes
src/cli.rs                     clap Command built at runtime from BUNDLES; CliOverrides
src/config/mod.rs              Config, load(), resolve()
src/config/file.rs             serde types for config.toml
src/config/hostmatch.rs        globset matching against /proc/sys/kernel/hostname
src/config/merge.rs            layering: defaults -> globals -> host tables -> CLI
src/platform.rs                Platform detection + Platforms bitflags
src/bundles/mod.rs             Bundle / Pkg / Provider types
src/bundles/catalog.rs         the const BUNDLES array
src/managers/mod.rs            Manager trait + registry
src/managers/{brew,npm,bun,flatpak,dnf}.rs
src/plan.rs                    PURE plan builder
src/execute.rs                 plan runner
src/dotfiles.rs                chezmoi bootstrap
src/report.rs                  output formatting
tests/cli.rs                   assert_cmd + insta snapshots
```

## Implementation detail

### Catalog (`src/bundles/`)

```rust
pub struct Bundle { name, description, default_enabled, platforms, packages }
pub struct Pkg    { name, providers: &'static [Provider] }
pub struct Provider {
    manager:   ManagerId,
    id:        &'static str,
    tap:       Option<&'static str>,   // brew third-party taps
    platforms: Platforms,
}

pub const BUNDLES: &[Bundle] = &[ /* core, dev, security, aws, home-assistant, desktop */ ];
```

Providers are **ordered by preference per package** — that is how "brew first,
dnf last resort" is encoded declaratively. Resolution walks providers and takes
the first whose manager is available *on this platform*. None →
`Unavailable { reason }`, warned and skipped; exit 2 under `--strict`.

`BUNDLES` is the single source of truth: CLI flags, `nt bundles`, and the
future TUI list all iterate it. Adding a bundle adds its flags automatically —
parity is structural, drift impossible.

#### Bundle taxonomy

| Bundle | Default | Platforms | Purpose |
|---|---|---|---|
| `core` | on | all | Terminal essentials assumed by the dotfiles |
| `dev` | on | all | Source control, editors, language tooling |
| `security` | on | all | Scanners and linters from the `security-scanning` skill |
| `aws` | off | all | AWS CLI and SSO helpers |
| `home-assistant` | off | all | `hass-cli` and friends |
| `desktop` | off | `!wsl` | GUI applications via flatpak |

#### Seed catalog (~12 packages, chosen adversarially)

Each entry exists to exercise a distinct framework path. Do not replace these
with an arbitrary sample — the coverage is the point.

| Package | Provider | Path it exercises |
|---|---|---|
| `bat`, `fd`, `ripgrep`, `eza`, `zoxide` | brew | The ordinary case; bulk set-diff |
| `chezmoi` | brew | Also required by the dotfiles step |
| `gh`, `git-delta` | brew | Name ≠ binary name (`git-delta` → `delta`) |
| `powertmux` | brew, `tap: powertmux/powertmux` | **Third-party tap** — must `brew tap` first |
| `trivy`, `gitleaks` | brew | `security` bundle populated |
| `homeassistant-cli` | brew | Off-by-default bundle excluded from the plan |
| `openclaw` | npm `openclaw` | **npm global manager** |
| `remmina` | flatpak `org.remmina.Remmina` | **flatpak**, `desktop` bundle, `!wsl` gate |
| `spotify` | flatpak `com.spotify.Client` | **Already `--system`-installed** — must read as present, not reinstall |
| `xdotool` | dnf `xdotool`, `platforms: !atomic` | **No user-space provider on atomic** → `Unavailable`, warn, skip, exit 2 under `--strict` |

The last two are the highest-value entries in the table: they are the two cases
that a naively-built framework gets wrong.

Once the framework is green against this seed, bulk-filling the remaining ~35
brew leaves and the curated flatpak subset is pure data entry with no design
risk. That is a follow-up task, not part of this plan.

### Managers (`src/managers/`)

```rust
trait Manager {
    fn id(&self) -> ManagerId;
    fn available(&self, p: &Platform) -> bool;      // PATH *and* platform gate
    fn installed(&self) -> Result<HashSet<String>>; // ONE bulk query
    fn install_cmd(&self, pkgs: &[String]) -> Command;
    fn upgrade_cmd(&self, pkgs: &[String]) -> Command;
}
```

`installed()` is deliberately **bulk** — one query per manager. Idempotency
becomes a set-diff, and a no-op `nt apply` costs *O(managers)* subprocesses
rather than *O(packages)*. This is what makes a routine run near-instant, and
it is also the seam for `FakeManager` in tests.

Per-manager specifics, all three driven by the inventory findings above:

- **brew** — `installed()` uses `brew list --formula -1` (the full 189, since
  for presence-checking a transitive dep still counts as installed).
  `nt status` separately uses `brew leaves` so its report reflects what was
  actually requested. Before installing a provider carrying a `tap`, run
  `brew tap <tap>` if absent. (**Superseded** - see the addendum: Linuxbrew
  casks do work, and are handled by a separate manager.)
- **flatpak** — `installed()` queries **both** `--user` and `--system` and
  unions the results, so the 106 existing system installs read as present.
  New installs use `--user`, so `nt` never touches system state.
- **dnf** — `available()` returns `false` whenever `platform.atomic`,
  regardless of `PATH`. This is the single most important gate in the codebase:
  `dnf` *is* on `PATH` under Bluefin and will appear to work.
- **npm** — `npm ls -g --depth=0 --json`, parsed for top-level keys.
- **bun** — absent on this machine; simply reports unavailable.

### Config resolution (`src/config/`)

Precedence, lowest to highest:
`Bundle::default_enabled` → `[bundles]` globals → each matching `[host."glob"]`
**in file order** → CLI flags.

`host` deserializes as an ordered `toml::Table` (requires the `preserve_order`
feature); each key compiles to a `globset::Glob` matched against the hostname.
Hostname comes from `/proc/sys/kernel/hostname`, overridable via `NT_HOSTNAME`.

Config path: `$XDG_CONFIG_HOME/napalm-tools/config.toml`, defaulting to
`~/.config/napalm-tools/config.toml`. A missing file is not an error — it means
"all defaults".

```toml
[bundles]
core = true
dev  = true
home-assistant = false

[extra]                     # escape hatch, no recompile
brew = ["jless", "dust"]

[options]
upgrade = false
strict  = false

[dotfiles]
enabled = true
repo    = "https://github.com/napalm255/dotfiles"
apply   = true

[host."*.naponline.net"]
bundles = { home-assistant = true, desktop = true }

[host."wsl-*"]
bundles = { desktop = false }
```

### Platform detection (`src/platform.rs`)

```rust
pub struct Platform { pub fedora_family: bool, pub atomic: bool, pub wsl: bool }
```

- `atomic` ← `/run/ostree-booted` exists
- `wsl` ← `WSL_DISTRO_NAME` set, or `/proc/sys/kernel/osrelease` contains `microsoft`
- `fedora_family` ← `ID` or `ID_LIKE` in `/etc/os-release` contains `fedora`

`os-release` path overridable via `NT_OS_RELEASE` so detection is testable
against fixtures.

### CLI (`src/cli.rs`)

```
nt apply [--dry-run] [--upgrade] [--strict] [--<bundle>|--no-<bundle>]… [--no-dotfiles]
nt status              what's installed vs. desired; changes nothing
nt bundles             the catalog, with effective state for this host
nt config show|path    resolved effective config  (bare `nt config` reserved for the TUI)
nt version             bare "0.1.0" to stdout, no TUI, pipe-friendly
nt completions <shell>
```

Built with the clap **builder** API so bundle flags can be emitted by iterating
`BUNDLES` at runtime — no `build.rs`, no codegen. Globals: `-v/-vv`, `--quiet`,
`--config <path>`.

Note `nt version` (bare `0.1.0`) is distinct from clap's `nt --version`
(`nt 0.1.0`); keep both.

### Dotfiles (`src/dotfiles.rs`)

After packages converge: ensure `chezmoi` is present, then
`chezmoi init --apply <repo>` when `~/.local/share/chezmoi` is absent, else
`chezmoi apply`. Honours `--dry-run` by rendering the command instead of
running it, and `--no-dotfiles` / `[dotfiles] enabled = false`.

### Errors and logging

`anyhow` with context at every boundary; subprocess failures carry the captured
stderr tail. `tracing` + `tracing-subscriber` behind `-v/-vv`.
Exit codes: `0` ok · `1` error · `2` unmet packages under `--strict`.

### Dependencies

`clap` + `clap_complete` · `serde` · `toml` (`preserve_order`) · `anyhow` ·
`tracing` + `tracing-subscriber` · `globset` · `anstream` / `anstyle`.
No `ratatui` yet. Hostname and platform detection are std-only `/proc` and
`/etc` reads — no `hostname` crate.

## Testing

Lean on the pure core:

- **Unit** — table-driven host-pattern merge cases (order, ties, no match,
  `*` catch-all); platform detection against fixture `os-release` strings;
  plan-building against a `FakeManager` snapshot. No subprocesses.
- **Integration** — hermetic via `NT_HOSTNAME` / `NT_CONFIG` / `NT_OS_RELEASE`,
  using `assert_cmd` with `insta` snapshots of `nt bundles` and
  `nt apply --dry-run`.

Follow TDD: write the failing test for each unit before its implementation.

## Verification

```bash
just fmt lint test                  # cargo fmt --check, clippy -D warnings, cargo test
cargo build --release

./target/release/nt version         # => bare "0.1.0"
./target/release/nt bundles         # catalog + effective state for this host
./target/release/nt config show     # merged config, host overrides applied
./target/release/nt apply --dry-run # plan only
./target/release/nt apply --dry-run --no-home-assistant   # bundle excluded from plan
NT_HOSTNAME=wsl-foo ./target/release/nt config show       # wsl-* overrides applied
./target/release/nt completions bash | bash -n            # completions parse
```

The seed catalog exists to make these five assertions meaningful on this
machine. Check each explicitly:

1. `nt apply --dry-run` emits **no `dnf` action at all** — `xdotool` appears
   instead as `unavailable: no user-space provider on atomic`.
2. `nt apply --dry-run --strict` exits **2** because of that `xdotool` line.
3. `com.spotify.Client` is reported **already installed** — proving the
   flatpak check reads `--system`, not just `--user`.
4. `powertmux` produces a `brew tap powertmux/powertmux` step ahead of its
   install, and no tap step on a re-run.
5. The `desktop` bundle vanishes from the plan under `NT_HOSTNAME=wsl-foo`
   combined with a WSL fixture `NT_OS_RELEASE`.

End-to-end: on this Bluefin machine `nt apply` installs only genuinely missing
packages, and a second `nt apply` reports zero actions and returns promptly.

## Explicitly out of scope

Immediate follow-up, no design risk, not part of this plan: bulk-filling the
catalog with the remaining ~35 brew leaves, the 4 npm globals, and a curated
subset of the 106 flatpaks. Pure data entry once the framework is green
against the seed.

Deferred to their own brainstorm cycles: the compiled-in script/plugin system
replacing `~/bin`, `nt --shell-init` replacing `.bashrc.d/`, and the ratatui
TUI (fastfetch-style screen with the animated fire effect). The bare
`nt config` invocation is reserved now so the TUI can claim it later without a
breaking change.

## Divergences from this design, found during implementation

1. **The dotfiles step became part of the plan.** The design treated it as a
   separate step run after the plan. In practice that meant its commands were
   printed after the "Unavailable" section, reading as part of it, and it meant
   dry-run and execute did not in fact share one path for the whole run.
   `ActionPlan` now carries a `dotfiles: Vec<Cmd>` field.

2. **Bundle flags extend to `nt bundles` and `nt config show`.** The design
   attached them to `apply` and `status` only, which meant `nt bundles --aws`
   failed while `nt apply --aws` worked. Any command that reports resolved
   state now accepts the overrides that change it.

3. **Two more environment overrides.** `NT_OSTREE_MARKER` and `NT_CONFIG` were
   added alongside `NT_HOSTNAME` and `NT_OS_RELEASE`. Without the first, the
   non-atomic code path cannot be exercised on an atomic development machine;
   without the second, integration tests read the developer's real config.

4. **No colour output.** `anstream` and `anstyle` were dropped from the
   dependency list; `report` emits plain text. Colour can be added later
   without changing the interface.

5. **`serde_json` added.** Needed to parse `npm ls -g --json`.

6. **Unknown bundle names are rejected.** Not stated in the design; a typo in
   `[bundles]` now fails loudly rather than silently doing nothing.

---

# Addendum: catalog design, 2026-08-25

## Correction: Linuxbrew casks do work

The original design stated twice that Linuxbrew has no cask support, and a
code comment in `src/managers/brew.rs` repeated it. **This was wrong.** The
development machine has twelve casks installed. Casks whose artifact is a
plain binary install on Linux; casks shipping an application bundle or a pkg
installer do not.

Two consequences:

1. The Homebrew **formula** `copilot` is the *AWS* ECS/Fargate tool, and its
   upstream `aws/copilot-cli` is **archived**. GitHub Copilot CLI is the
   **cask** `copilot-cli`. Cataloguing the formula would have installed the
   wrong, unmaintained software.
2. Google Antigravity is installable after all, via the cask
   `antigravity-cli`. An earlier proposal for a "manual install" note field
   was therefore dropped: it had no remaining user.

`ManagerId::BrewCask` exists as a manager separate from `ManagerId::Brew`
precisely because of point 1. Formula and cask names are different namespaces
that genuinely collide, and separate installed-sets remove the ambiguity
rather than papering over it.

## Binary-presence satisfaction

`Pkg` gained `binary: Option<&'static str>`. If the named executable resolves
on `PATH`, the package is satisfied whatever installed it. A manager that owns
the package still takes precedence, so `--upgrade` remains meaningful.

This addresses three real cases found on the development machine:

- The OS image already ships `jq`, `git`, `vim`, `fzf`, `tmux`, `htop`, `just`
  and more. Without the rule, `nt` installs brew copies that shadow them.
- Claude Code is installed by its own vendor script into `~/.local/bin`, where
  it self-updates. Without the rule, `nt` adds a second, staler copy via npm.
- Formula names differ from binary names: `ripgrep`/`rg`, `git-delta`/`delta`,
  `tealdeer`/`tldr`, `miller`/`mlr`, `difftastic`/`difft`.

Measured on the development machine: the rule prevents eight redundant
installs on a default `nt apply`.

## Dependency vetting

All catalog packages and the project's own dependencies were checked
against the project rules: at least 1000 GitHub stars, a commit within six
months, not archived, compatible licence, with the first-party carve-out for
tooling published by a language or platform owner.

### Rejected, and why - do not reintroduce

| Package | Reason |
|---|---|
| `copilot` (formula) | Archived upstream, and the wrong software entirely |
| `html2text` | No commit in ten months. Replaced by `pandoc` |
| `pup` | Original two years stale; the maintained DataDog fork has 994 stars, under the threshold. Replaced by `htmlq` |
| `tree` | 323 stars, and an OS-native package - the ladder stops before Homebrew. `eza --tree` covers it |
| `antigravity` (npm) | **Not Google's.** Version 0.0.0, description "placeholder for the haters", unrelated maintainer. A supply-chain hazard |
| `netcat` (formula) | GNU netcat 0.7.1, released 2004 and dormant since. Distributions ship a current one, so the package resolves through the binary check and falls back to dnf |
| `telnet` (formula) | A port of Apple's `remote_cmds` with no bottle at all, so it would build from source on Linux. GNU `inetutils` supplies telnet instead |

### Accepted exceptions

| Package | Gate missed | Justification |
|---|---|---|
| `govulncheck` | 510 stars | First-party carve-out: the Go team's own vulnerability scanner, named explicitly in the project rules |
| `assert_cmd` | 560 stars | Dev-dependency only, never shipped. Maintained under `assert-rs` by the maintainer of `clap` and `toml`, both already dependencies here. Active within the week |
| `predicates` | 211 stars | Same maintainer, same recency, also dev-only |

Licences across the catalog are permissive (MIT, Apache-2.0, BSD) or copyleft
on a tool that is merely executed, never linked - `shellcheck` (GPL-3.0),
`golangci-lint` (GPL-3.0), `semgrep` (LGPL-2.1), `pandoc` (GPL-2.0),
`eza` (EUPL-1.2), `tmux`/`git` (GPL/BSD). None are linked into `nt`.

## Bundle taxonomy

Seventeen bundles, ninety-nine packages. `core`, `shell`, `security` and `ai` are
on by default; everything else is opt-in. Language tooling is one bundle per
language so a machine that never touches Go can skip that tooling entirely.
Language **runtimes** are separate opt-in bundles, because `mise` manages
runtimes where it is in use and two things managing one runtime means `PATH`
order decides the winner.

Selection filter, applied throughout: machine-readable output (JSON or SARIF),
non-interactive, precise `file:line:col`, meaningful exit codes, single static
binary. That filter is why `ruff`, `biome`, `oxlint` and `uv` are preferred
over their predecessors, and why the interactive SQLite front-ends
(`litecli`, `harlequin`, `visidata`, `dblab`) were considered and rejected.

## Binary names are the catalog's silent failure mode

A wrong `binary` is invisible: the package installs, the presence check never
matches it, and `nt` reinstalls it whenever the owning manager happens not to
report it. Nothing errors.

`scripts/audit-binaries.py` (`just audit`) compares each declaration against
the machine's real state and reports any package a manager calls installed
whose declared binary is absent. It found one such case on first run: the
`antigravity-cli` cask installs its binary as `agy`, because the cask artifact
line reads `antigravity -> agy` - the file `antigravity` is installed *as*
`agy`. Eighty-five other declarations were correct.

The audit can only check what is currently installed, so it is a net rather
than a proof.
