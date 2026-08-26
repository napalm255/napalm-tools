set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes
default:
    @just --list

# Install the toolchain components and dev tools this project expects
setup:
    rustup component add rustfmt clippy
    @echo "Optional scanners (install via brew): osv-scanner gitleaks trivy"

# Format code in place
fmt:
    cargo fmt

# Static analysis; changes nothing
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# Run the test suite
test *args:
    cargo test {{ args }}

# Full local security scan; missing scanners are skipped, not fatal
security:
    #!/usr/bin/env bash
    set -euo pipefail
    ran=0
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

# Check catalog binary names against what is actually installed
audit:
    ./scripts/audit-binaries.py

# Everything CI runs, in order
ci: lint test security

# Regenerate shell completions into ./completions
completions: build
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p completions
    for shell in bash zsh fish; do
        ./target/release/nt completions "$shell" > "completions/nt.$shell"
    done
    echo "wrote completions/nt.{bash,zsh,fish}"
