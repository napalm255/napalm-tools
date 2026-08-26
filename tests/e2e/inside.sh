#!/usr/bin/env bash
# Runs inside the container, as root at first: creates the test user if the
# image lacks one, then re-executes itself as that user for the real test.
set -euo pipefail

user="${E2E_USER:-dev}"
export PATH="/var/tmp/nt-e2e:$PATH"

if [ "$(id -u)" = 0 ]; then
  if ! id "$user" >/dev/null 2>&1; then
    useradd --create-home --shell /bin/bash "$user"
    echo "$user ALL=(ALL) NOPASSWD:ALL" > "/etc/sudoers.d/$user"
    chmod 0440 "/etc/sudoers.d/$user"
  fi
  # Everything nt needs to reach the network and unpack bottles.
  if command -v dnf >/dev/null && [ ! -e /run/ostree-booted ] && [ -z "${NT_OSTREE_MARKER:-}" ]; then
    dnf install -y -q sudo git curl file procps-ng util-linux which tar gzip xz >/dev/null 2>&1 || true
  fi
  # A booted system gives /tmp the sticky world-writable mode; the OCI image
  # alone does not, and rustup-init needs somewhere to mktemp.
  chmod 1777 /tmp
  # Root must be refused before anything else is tried.
  if nt apply --dry-run >/dev/null 2>&1; then
    echo "FAIL: nt apply ran as root" >&2; exit 1
  fi
  echo "ok: apply refuses root"
  exec sudo -u "$user" -E -H bash "$0"
fi

export HOME; HOME="$(getent passwd "$(id -un)" | cut -d: -f6)"
export PATH="$HOME/.local/bin:/home/linuxbrew/.linuxbrew/bin:$PATH"
export NT_CONFIG="$HOME/nt-e2e.toml"

# The devcontainer image ships mise for development. Hide it so the run
# exercises the real bootstrap path: Homebrew first, then mise from Homebrew.
rm -f "$HOME/.local/bin/mise"
hash -r
if command -v mise >/dev/null; then
  echo "FAIL: mise is still on PATH; the bootstrap path would not be tested" >&2; exit 1
fi

only=()
if [ "${E2E_FULL:-0}" != 1 ]; then
  for b in ${E2E_BUNDLES:-core shell prompt}; do only+=(--only "$b"); done
fi

cat > "$NT_CONFIG" <<TOML
[dotfiles]
enabled = false
TOML

echo "== version"; nt version
echo "== platform"; nt config show | head -3
echo "== catalog"; nt bundles --detail "${only[@]}" | head -40

echo "== dry run"
nt apply --dry-run --output json "${only[@]}" > "$HOME/e2e-plan.json"
jq -r '.actions[] | "\(.kind)\t\(.command)"' "$HOME/e2e-plan.json"
jq -e '.actions | map(select(.kind == "bootstrap")) | length > 0' "$HOME/e2e-plan.json" >/dev/null \
  || { echo "FAIL: a fresh host should plan a bootstrap" >&2; exit 1; }
jq -e '.actions | map(select(.kind == "bootstrap" and (.command | test("Homebrew/install")))) | length == 1' "$HOME/e2e-plan.json" >/dev/null \
  || { echo "FAIL: the bootstrap should install Homebrew" >&2; exit 1; }
jq -e '.actions | map(select(.command == "brew install mise")) | length == 1' "$HOME/e2e-plan.json" >/dev/null \
  || { echo "FAIL: the bootstrap should install mise from Homebrew" >&2; exit 1; }

echo "== apply"
time nt apply --output plain "${only[@]}"

echo "== verify"
hash -r
for b in brew mise rg fd bat starship; do
  command -v "$b" >/dev/null || { echo "FAIL: $b not found after apply" >&2; exit 1; }
done
echo "ok: brew=$(command -v brew) mise=$(command -v mise) rg=$(rg --version | head -1)"
eval "$(nt shell-init bash)" && echo "ok: shell-init evaluates"

echo "== status"
nt status "${only[@]}"
nt status --output json "${only[@]}" > "$HOME/e2e-status.json"
missing="$(jq '.totals.missing' "$HOME/e2e-status.json")"
[ "$missing" = 0 ] || { echo "FAIL: $missing packages still missing" >&2; jq '.packages[] | select(.state=="missing")' "$HOME/e2e-status.json"; exit 1; }

echo "== second apply must be a no-op"
nt apply --dry-run --output json "${only[@]}" > "$HOME/e2e-plan2.json"
count="$(jq '.actions | length' "$HOME/e2e-plan2.json")"
[ "$count" = 0 ] || { echo "FAIL: second run planned $count actions" >&2; jq '.actions' "$HOME/e2e-plan2.json"; exit 1; }
echo "ok: converged"
