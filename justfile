set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes
default:
    @just --list

# Install the dev tools this project expects (toolchain comes from mise.toml)
setup:
    cargo binstall -y --locked cargo-deny cargo-audit
    @echo "Optional scanners (brew install): osv-scanner gitleaks trivy"

# Format code in place
fmt:
    cargo fmt

# Static analysis; changes nothing
lint:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings

# Run the unit and integration tests
test *args:
    cargo test --all-features {{ args }}

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
        echo "no scanners installed; see 'just setup'" >&2
        exit 1
    fi

# Build the release binary
build:
    cargo build --release

# Run the binary
run *args:
    cargo run -- {{ args }}

# Remove build output
clean:
    cargo clean

# Check catalog binary names against what is actually installed here
audit-binaries: build
    ./scripts/audit-binaries.py

# Everything CI runs before the container jobs, in order
ci: lint test security

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
