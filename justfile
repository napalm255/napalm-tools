set shell := ["bash", "-euo", "pipefail", "-c"]

# Rust comes from rustup (rust-toolchain.toml); just and cargo-binstall from
# mise. Put both on PATH so every recipe works in a shell that has activated
# neither, once `just setup` has run.
export PATH := env("HOME") / ".cargo/bin" + ":" + env("HOME") / ".local/share/mise/shims" + ":" + env("PATH")

# List available recipes
default:
    @just --list

# The shell scripts this repository owns, for linting
shell_files := "scripts/clean.sh tests/e2e/run.sh tests/e2e/inside.sh tests/fixtures/fake-bin/fake"

# Line coverage the test suite must keep. Set from the measured baseline;
# raise it as coverage grows, never lower it to get a build through.
coverage_floor := "94"

# Install the toolchain (rust-toolchain.toml) and every dev tool (mise.toml)
setup:
    mise trust --quiet
    mise install --yes
    rustup show active-toolchain || rustup toolchain install

# Format code in place
fmt:
    cargo fmt

# Static analysis of everything; changes nothing
lint: lint-rust lint-scripts lint-config

# Rust formatting and clippy, warnings as errors
lint-rust:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings

# Shell and Python scripts
lint-scripts:
    shellcheck {{ shell_files }}
    shfmt -d -i 2 -ci {{ shell_files }}
    ruff check scripts
    ruff format --check scripts

# Workflows, YAML, spelling
lint-config:
    actionlint
    zizmor --min-severity low .github/workflows
    yamllint --strict .github .yamllint
    typos

# Run the unit and integration tests
test *args:
    cargo test --all-features {{ args }}

# Run the tests under coverage, write target/lcov.info, and fail below the floor
coverage:
    cargo llvm-cov --all-features --workspace --lcov --output-path target/lcov.info --fail-under-lines {{ coverage_floor }}
    cargo llvm-cov report --summary-only

# Browse the last coverage run as HTML
coverage-html:
    cargo llvm-cov report --html --open

# Dependency policy: advisories, licences, bans, sources
deny:
    cargo deny check

# RustSec advisories
audit:
    cargo audit

# Full local security scan; missing scanners are skipped, not fatal
security:
    #!/usr/bin/env bash
    set -euo pipefail
    ran=0
    if command -v cargo-audit >/dev/null 2>&1; then
        echo "== cargo audit =="; cargo audit; ran=1
    fi
    if command -v cargo-deny >/dev/null 2>&1; then
        echo "== cargo deny =="; cargo deny check; ran=1
    fi
    if command -v osv-scanner >/dev/null 2>&1; then
        echo "== osv-scanner =="; osv-scanner scan source .; ran=1
    fi
    if command -v gitleaks >/dev/null 2>&1; then
        echo "== gitleaks =="; gitleaks detect --no-banner --redact; ran=1
    fi
    if command -v trivy >/dev/null 2>&1; then
        echo "== trivy =="; trivy fs --scanners vuln,secret,misconfig --exit-code 1 .; ran=1
    fi
    if [ "$ran" -eq 0 ]; then
        echo "no scanners installed; run 'just setup'" >&2
        exit 1
    fi

# Build the release binary
build:
    cargo build --release

# Run the binary
run *args:
    cargo run -- {{ args }}

# Remove everything this repo created: build output, completions, e2e/devcontainer images
clean *args:
    ./scripts/clean.sh {{ args }}

# Check catalog binary names against what is actually installed here
audit-binaries: build
    ./scripts/audit-binaries.py

# Everything CI runs before the container jobs, in order (coverage runs the tests)
ci: lint coverage security

# Build the release archive and checksum into dist/; a tag must match Cargo.toml
release-assets tag="": build completions
    #!/usr/bin/env bash
    set -euo pipefail
    version="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')"
    if [[ -n "{{ tag }}" && "{{ tag }}" != "v${version}" ]]; then
        echo "tag {{ tag }} does not match Cargo.toml version ${version}" >&2
        exit 1
    fi
    name="nt-v${version}-x86_64-unknown-linux-gnu"
    rm -rf dist
    mkdir -p "dist/${name}"
    cp target/release/nt LICENSE README.md "dist/${name}/"
    cp completions/nt.bash completions/nt.zsh completions/nt.fish "dist/${name}/"
    tar -C dist -czf "dist/${name}.tar.gz" "${name}"
    (cd dist && sha256sum "${name}.tar.gz" > "${name}.tar.gz.sha256")
    rm -rf "dist/${name}"
    ls -l dist

# Build the devcontainer image (also the Fedora end-to-end image)
image:
    podman build -t napalm-tools-dev -f .devcontainer/Containerfile .

# End-to-end: a real `nt apply` inside the Fedora devcontainer image
e2e-fedora: build
    ./tests/e2e/run.sh fedora

# End-to-end: a real `nt apply` inside a Bluefin image
e2e-bluefin: build
    ./tests/e2e/run.sh bluefin

# Both end-to-end suites
e2e: e2e-fedora e2e-bluefin

# Regenerate shell completions into ./completions
completions: build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p completions
    for shell in bash zsh fish; do
        ./target/release/nt completions "$shell" > "completions/nt.$shell"
    done
    echo "wrote completions/nt.{bash,zsh,fish}"
