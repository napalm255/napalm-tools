# From bundle toggles to an opinionated setup tool - design

*Status: implemented. Written 2026-08-25 as the record of an adversarial
review of the v0.1 foundation and of the decisions that reshaped it.*

## Brief

The user asked for an adversarial pass over everything to date - bugs,
security, duplication, gaps in tests, logging - and for the tool to become
**opinionated**: every language and its toolset installed, always, on Fedora
workstation, server, the official container image and WSL, as well as on
Bluefin. Specific additions: Amazon Corretto Java, Android development,
PowerShell, and a shell prompt (starship by default, powerbash or oh-my-posh
selectable). Also: a devcontainer that runs the whole test suite, automated
tests against both Fedora and Bluefin, mise for the project itself, an
`AGENTS.md`, a `--detail` view of what a bundle contains, a coherent `nt
status`, a more colourful CLI, and help that only shows options where they
work.

## Audit findings

Everything below was confirmed by reading the code or by running it, not
assumed. Each is fixed in this change unless marked otherwise.

### Wrong

1. **`nt status` measured the wrong thing.** It printed each manager's
   *entire* installed set - `brew leaves`, and for flatpak the full
   `flatpak list --system` including runtimes and extensions (106 rows here,
   63 of them applications, none of them catalog entries). The "88 flatpaks"
   the user could not explain were the machine's runtimes, deduplicated. The
   count had no relation to the catalog. It also re-ran every manager's
   listing a second time to get those numbers, doubling the cost of the
   command. `status` now reports desired-versus-installed, per bundle, from
   the one snapshot.
2. **`flatpak install --user` had no remote to install from.** The user
   scope on this machine, and on any fresh Fedora, has no remotes; only the
   system scope has Flathub. Every flatpak install would have failed. The
   plan now adds Flathub to the user scope first.
3. **Nothing bootstrapped Homebrew or mise.** On Bluefin both are
   preinstalled, so the gap was invisible. On Fedora, WSL and the container
   image `brew` is absent, so every catalog package would have been reported
   unavailable. `apply` now has a bootstrap phase.
4. **Colour leaked into redirected stdout.** `Format::detect` looked at
   *stderr*, so `nt bundles > file` wrote escape sequences into the file
   whenever stderr was a terminal. Stdout decoration now follows stdout;
   progress on stderr follows stderr.
5. **chezmoi scripts in `.chezmoiscripts/` were never scanned for `sudo`.**
   That subdirectory is chezmoi's recommended location for run scripts, so
   the privilege pre-check missed the common case.
6. **`Cmd::output()` skipped the program-exists check** that `run_captured`
   performs, so a missing manager binary surfaced as a confusing `setsid`
   exit status rather than "not installed".
7. **`on_path` accepted any regular file**, executable or not.
8. **`[extra]` names were passed to managers unvalidated.** A value such as
   `"--force"` would have become a flag on the install command. Names are
   now validated at the boundary: no leading `-`, no whitespace, non-empty.
   The same check covers the dotfiles repository.
9. **`$HOME` unset fell back to a relative path** for the chezmoi source
   directory. It is now an error.

### Duplicated

- Every manager built its install command with the same
  `vec![...]; args.extend(...)` dance; four managers wrapped
  `parse_lines` in an identically-named `parse_list`. Replaced by
  `Cmd::with_packages` and direct use of `parse_lines`.
- `execute::snapshot` iterated the manager registry twice and called
  `available()` twice per manager.
- Platform constants (`ATOMIC`, `PLAIN`, `UNDER_WSL`) were redefined in six
  test modules. They now live in `platform::test_platforms`.

### Missing

- No test covered a bundle whose packages span two managers being planned
  as two commands, the `--upgrade` path for extras, or JSON output of
  `status`.
- No integration test ran the binary on anything but the developer's own
  host state. The end-to-end suite now runs `nt apply` for real inside a
  Fedora container and a Bluefin container.
- No `AGENTS.md`, no devcontainer, no `mise.toml`, no CI.

### Not changed, and why

- Trusting a third-party Homebrew tap automatically (`brew trust`) runs
  code from that tap. The only tapped entries are the user's own taps, and
  the action is visible in `--dry-run`. Documented; not removed.
- `GIT_SSH_COMMAND=ssh -o BatchMode=yes` is set for every subprocess so a
  clone cannot hang on a passphrase prompt. It still honours `~/.ssh/config`.

## Decisions

### The catalog is opinionated

Bundles remain as the *structure* of the catalog - they are how `nt bundles
--detail` groups packages and how `status` reports - but every bundle is on
by default. The only things that turn a bundle off are:

- the platform: `desktop` and `fonts` need a graphical session and are
  skipped on WSL, in a container, and on a server;
- `[bundles] name = false` in the configuration, for the few people who do
  not want, say, Android;
- `--skip <bundle>` and `--only <bundle>` on `apply`, `status` and
  `bundles`, for one run.

The thirty-four generated `--<bundle>` / `--no-<bundle>` flags are gone.
They swamped `--help`, and "enable this one bundle for this run" is what
`--only` says more clearly.

### Two managers, each for what it is good at

- **Homebrew** for command-line tools. Bottles for Linux, no sudo, and the
  broadest coverage of the tools in the catalog.
- **mise** for language toolchains and anything that wants a JDK: Java
  (Corretto), Kotlin, Gradle, Maven, ktlint, Node, Bun, Deno, Python, Go,
  Rust, Ruby, Zig, .NET, PHP, Lua, Perl, Elixir/Erlang, and the Android
  command-line tools. mise pins versions per user and keeps Homebrew's own
  OpenJDK out of the picture; the Kotlin, Gradle and Maven formulae would
  otherwise each pull it in beside Corretto.

mise is a first-class `Manager`. Its provider id is `tool@version`
(`java@corretto-21`, `node@lts`, `python@3.13`), it lists what is installed
with `mise ls --global --json`, and it installs with `mise use --global`.

PowerShell, starship and oh-my-posh come from Homebrew, which now ships
Linux bottles for all three. powerbash comes from its tap.

`dnf` remains the last resort, reachable only through `[extra]` or where
Homebrew cannot serve, and never on an atomic host.

### Bootstrap before snapshot

`apply` runs in three phases: bootstrap, snapshot, converge.

Bootstrap installs what the managers themselves need and which no manager
can provide: Homebrew on a host without it (its prerequisites via `dnf`,
then the official installer), mise where Homebrew is present but mise is
not, and the Flathub remote in the user scope when flatpak is usable. Each
step is planned only when its check fails, is shown by `--dry-run`, and is
run before the snapshot so the snapshot sees the managers it just made
available.

`nt` refuses to run `apply` as root. Homebrew refuses root too, and a
root-owned `~/.local` is a trap for every later tool. The container tests
run as an ordinary user with passwordless sudo, which is also what the
devcontainer does.

### Platform gains two facts

`Platform` now records `container` (`/run/.containerenv` or `/.dockerenv`
exists) and `graphical` (a desktop session is installed:
`/usr/share/wayland-sessions` or `/usr/share/xsessions` has entries).
Environment variables such as `DISPLAY` were rejected as the signal because
they are absent from the very shells - SSH, agents, cron - that most often
run a setup tool.

### Shell prompt

`[shell] prompt = "starship" | "oh-my-posh" | "powerbash"`, default
`starship`. The chosen prompt is added to the plan; the others are not.
`nt shell-init <shell>` prints the line that activates it, so dotfiles can
contain `eval "$(nt shell-init bash)"` and the choice lives in one place.
powerbash is bash-only and `shell-init` says so for any other shell.

### `status` and `--detail`

`nt status` takes the snapshot once and reports, per bundle, how many
packages are installed, missing, or unavailable, with a total. `--detail`
expands each bundle to one row per package showing the provider that would
be used and where the package was found (`brew`, `mise`, `on PATH`, ...).
`nt bundles --detail` shows the catalog's packages and providers without
touching the machine.

### CLI hygiene

Every flag is declared on the commands where it does something:

| flag | commands |
| --- | --- |
| `--config`, `--output`, `-v` | apply, status, bundles, config show |
| `--skip`, `--only` | apply, status, bundles |
| `--detail` | status, bundles |
| `--dry-run`, `--upgrade`, `--strict`, `--no-dotfiles`, `--prompt`, `-q` | apply |

`version`, `completions` and `shell-init` take no flags beyond their
argument. `--skip` and `--only` validate against the catalog, so
completions offer the names and a typo is an error.

### Presentation

The pretty theme uses colour and a small set of glyphs, and emoji headings
where the terminal is UTF-8: a package icon for bundles, a tick for
satisfied, an arrow for installs, a warning sign for unavailable. `plain`
and `json` are unchanged and byte-identical whatever the terminal.

### Project tooling

- `mise.toml` pins the Rust toolchain and installs `just`; `just` remains
  the task runner and gains `e2e-fedora` and `e2e-bluefin`.
- `.devcontainer/` builds from the official Fedora image, pinned by digest,
  installs mise as an ordinary user, and runs `just ci` on create.
- `tests/e2e/` holds a container-driven suite: build `nt`, run it inside
  Fedora and Bluefin images as a non-root user with passwordless sudo,
  and assert that a second `apply` is a no-op.
- `.github/workflows/ci.yml` runs lint, tests and both end-to-end jobs.
- `AGENTS.md` describes the repository for any agent working in it and
  carries the standing instruction that it and `README.md` are updated with
  every change that affects them.

## Catalog additions, vetted

Every addition was checked on 2026-08-25 against the project rules: at
least 1000 GitHub stars, a push within six months, not archived, a
compatible licence. Results are in the table in `AGENTS.md`; the notable
outcomes:

| Package | Outcome |
| --- | --- |
| Amazon Corretto | 121 stars on the `corretto-21` repo. First-party carve-out: Amazon publishes it, and it was requested by name |
| `markdownlint-cli2` | **Rejected**, 907 stars |
| `cpanminus` | **Rejected**, 782 stars and last push five months ago |
| `dive` | **Rejected**, last push December 2025 |
| `pipenv` | **Removed** from the catalog; `uv` covers it and two Python environment tools is one too many |
| Zig | GitHub mirror last pushed November 2025 because development moved to Codeberg, where it is active. Kept |
| Remmina | GitHub mirror last pushed February 2026; development is on GitLab and active. Kept |

## Out of scope

The compiled-in script system, the ratatui interface and `rpm-ostree`
layering remain deferred. Android SDK component installation is left to
Google's `android` CLI, which the Android bundle installs; it needs licence
acceptance that should be a deliberate act.

## Verification

Run on 2026-08-25 on the development machine (Bluefin 44).

- `just ci`: `cargo fmt --check`, `clippy --all-targets --all-features -D
  warnings`, 327 unit and 26 integration tests, `cargo audit`, `cargo deny
  check`, osv-scanner, gitleaks and trivy - all clean.
- `nt status` here: 84 of 133 catalog packages present, 49 to install; the
  dry run groups them into one `brew install`, one `npm install -g` and one
  `mise use --global`.
- `nt bundles > file` contains no escape sequences; under a pty the output
  is coloured with emoji headings.
- `just e2e-fedora` (the official `fedora:44` image, ordinary user with
  passwordless sudo, mise removed so the bootstrap path is real): root was
  refused; the dry run planned the Homebrew installer and `brew install
  mise`; `nt apply --only core --only shell --only prompt --only go --only
  rust` installed 60 packages in 2m04s; `nt status` reported 60 of 60
  present; a second dry run planned zero actions.
- `just e2e-bluefin` (`bluefin-dx:stable` 44.20260825 by digest, atomic
  marker set): root refused; bootstrap planned only the Homebrew installer
  and `brew install mise` - no dnf step; the same five bundles installed in
  1m13s; 60 of 60 present; second run planned nothing.
- The devcontainer image itself: `mise install`, `just setup`, `just lint`,
  `just test`, `just deny`, `just audit` all pass inside it, with
  `--userns=keep-id` so the bind-mounted checkout is writable.
- `mise` specs resolve: `java@corretto-21` -> corretto-21.0.12.9.1,
  `dotnet@10` -> 10.0.400, `node@lts` -> 24.19.0, `python@3.13` -> 3.13.15,
  `android-cli@latest` -> 1.0.15985488, which installs Google's unified
  `android` CLI binary (verified with `android --help`).

## Divergences found during implementation

1. **mise reads the current directory.** `mise ls --global` failed from
   inside this repository once it gained a `mise.toml`, because mise refuses
   untrusted project files before answering anything. `Cmd` gained a
   working directory and every mise command runs from `$HOME`.
2. **The probe line ended in a carriage return.** indicatif's `println`
   after the last spinner is cleared leaves the cursor on the same line, so
   the first line of stdout was glued to "checked N package managers". This
   was visible in the user's original paste. Completed-step lines now go
   straight to stderr.
3. **`android-cli` is not `cmdline-tools`.** mise's `android-cli` is
   Google's new unified `android` binary from `dl.google.com/android/cli`,
   which manages SDK components, emulators and projects itself. The catalog
   comment and README were corrected; it is the better first-party choice.
4. **`NT_TOOL_DIRS`.** The resolver looks in `/home/linuxbrew/.linuxbrew/bin`
   and `~/.local/bin` even when they are off `PATH`, so a manager installed
   by the bootstrap phase is found in the same run. That defeats a test's
   attempt to simulate a fresh host by emptying `PATH`, so the list is
   overridable.
5. **Container images are not booted systems.** The Bluefin image has no
   `/usr/local` target and a non-sticky `/tmp`; the e2e harness mounts
   under `/var/tmp` and sets `/tmp` to 1777 as a booted system would. The
   devcontainer needs `--userns=keep-id` under rootless podman so the
   container user owns the mounted checkout.
6. **trivy's `DS-0026` (HEALTHCHECK) is ignored** for the devcontainer image
   via `.trivyignore`; it is not a service.

## Follow-up, 2026-08-26

- `just clean` (`scripts/clean.sh`) removes what the repository created -
  `target/`, `completions/`, the devcontainer image and the digest-pinned
  e2e images, read from the files that name them - and nothing else.
- Step lines lead with the status (`✓ [3/4] brew install …`), so a long
  command cannot push the outcome off screen; live spinner text is cut to
  the terminal width, which stops stale frames being left behind when a
  wrapped line is cleared.
- A failed package step no longer aborts the run: every independent step
  runs, failures are listed at the end with their output, and the exit code
  is 1. A failed bootstrap still ends the run; dotfiles are skipped after a
  package failure.
- `pa11y` was dropped: puppeteer downloads its own Chrome at install time
  and could not repair a broken cache. Playwright (first-party Microsoft)
  takes its place, with a `playwright` manager that installs Chromium in
  user space only when no complete revision exists.
- The justfile puts mise's shims on `PATH` and `just setup` runs
  `mise install`, after `just build` failed in a shell without mise
  activated.
