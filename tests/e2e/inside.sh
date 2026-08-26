#!/usr/bin/env bash
# Runs inside the container, as root at first: creates the test user if the
# image lacks one, then re-executes itself as that user for the real test.
# Every phase ends in an `ok:` or `FAIL:` line, so a CI log can be scanned.
set -euo pipefail

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Run a check, naming it in the failure.
check() {
  local what="$1"
  shift
  "$@" || fail "${what}"
  echo "ok: ${what}"
}

# Which executables each bundle is expected to leave on PATH. Toolchains
# from mise live behind shims that this shell does not activate, so they
# are proven by `nt status` reporting nothing missing rather than listed
# here.
binaries_for() {
  case "$1" in
    core) echo "rg fd bat" ;;
    shell) echo "shellcheck shfmt" ;;
    prompt) echo "starship" ;;
    *) ;;
  esac
}

as_root() {
  local user="$1"
  if ! id "${user}" >/dev/null 2>&1; then
    useradd --create-home --shell /bin/bash "${user}"
    echo "${user} ALL=(ALL) NOPASSWD:ALL" >"/etc/sudoers.d/${user}"
    chmod 0440 "/etc/sudoers.d/${user}"
  fi
  # Everything nt needs to reach the network and unpack bottles.
  if command -v dnf >/dev/null && [[ ! -e /run/ostree-booted && -z ${NT_OSTREE_MARKER:-} ]]; then
    dnf install -y -q sudo git curl file procps-ng util-linux which tar gzip xz jq >/dev/null 2>&1 || true
  fi
  # A booted system gives /tmp the sticky world-writable mode; the OCI image
  # alone does not, and rustup-init needs somewhere to mktemp.
  chmod 1777 /tmp

  # Root must be refused before anything else is tried - and refused for
  # that reason, not because the binary failed to start at all.
  local out
  if out="$(nt apply --dry-run 2>&1)"; then
    fail "nt apply ran as root"
  fi
  grep -q "must run as an ordinary user" <<<"${out}" ||
    fail "nt apply as root failed for another reason: ${out}"
  echo "ok: apply refuses root"

  exec sudo -u "${user}" -E -H bash "$0"
}

as_user() {
  local full=0 only=() bundles b plan status count missing
  export HOME
  HOME="$(getent passwd "$(id -un)" | cut -d: -f6)"
  export PATH="${HOME}/.local/bin:/home/linuxbrew/.linuxbrew/bin:${PATH}"
  export NT_CONFIG="${HOME}/nt-e2e.toml"
  plan="${HOME}/e2e-plan.json"
  status="${HOME}/e2e-status.json"

  command -v jq >/dev/null || fail "jq is required inside the image"

  # The devcontainer image ships mise for development. Hide it so the run
  # exercises the real bootstrap path: Homebrew first, then mise from Homebrew.
  rm -f "${HOME}/.local/bin/mise"
  hash -r
  if command -v mise >/dev/null; then
    fail "mise is still on PATH; the bootstrap path would not be tested"
  fi

  case "${E2E_FULL:-0}" in
    1 | true | yes) full=1 ;;
  esac
  bundles="${E2E_BUNDLES:?E2E_BUNDLES must list the bundles to apply}"
  if [[ ${full} == 0 ]]; then
    for b in ${bundles}; do only+=(--only "${b}"); done
  fi

  cat >"${NT_CONFIG}" <<TOML
[dotfiles]
enabled = false
TOML

  echo "== version"
  check "version prints" nt version
  echo "== platform"
  check "config show renders" nt config show
  echo "== catalog"
  check "bundles renders" nt bundles --detail "${only[@]}"

  echo "== dry run"
  nt apply --dry-run --output json "${only[@]}" >"${plan}" || fail "dry run plans"
  echo "ok: dry run plans"
  jq -r '.actions[] | "\(.kind)\t\(.command)"' "${plan}"
  check "a fresh host plans a bootstrap" \
    jq -e '.actions | map(select(.kind == "bootstrap")) | length > 0' "${plan}"
  check "the bootstrap installs Homebrew" \
    jq -e '.actions | map(select(.kind == "bootstrap" and (.command | test("Homebrew/install")))) | length == 1' "${plan}"
  check "the bootstrap installs mise from Homebrew" \
    jq -e '.actions | map(select(.command == "brew install mise")) | length == 1' "${plan}"

  echo "== apply"
  local started=${SECONDS}
  check "apply converges" nt apply --output plain "${only[@]}"
  echo "ok: apply took $((SECONDS - started))s"

  echo "== verify"
  hash -r
  for b in brew mise; do
    command -v "${b}" >/dev/null || fail "${b} not found after apply"
  done
  if [[ ${full} == 1 ]]; then
    bundles="core shell prompt"
  fi
  for b in ${bundles}; do
    for bin in $(binaries_for "${b}"); do
      command -v "${bin}" >/dev/null || fail "${bin} (bundle ${b}) not found after apply"
    done
  done
  echo "ok: brew=$(command -v brew) mise=$(command -v mise) rg=$(rg --version | head -1)"
  eval "$(nt shell-init bash)"
  echo "ok: shell-init evaluates"

  echo "== status"
  check "status renders" nt status "${only[@]}"
  nt status --output json "${only[@]}" >"${status}" || fail "status renders as json"
  missing="$(jq '.totals.missing' "${status}")"
  [[ ${missing} == 0 ]] || {
    jq '.packages[] | select(.state=="missing")' "${status}"
    fail "${missing} packages still missing"
  }
  echo "ok: nothing missing"

  echo "== second apply must be a no-op"
  nt apply --dry-run --output json "${only[@]}" >"${plan}" || fail "second dry run plans"
  count="$(jq '.actions | length' "${plan}")"
  [[ ${count} == 0 ]] || {
    jq '.actions' "${plan}"
    fail "second run planned ${count} actions"
  }
  echo "ok: converged"
}

main() {
  export PATH="/var/tmp/nt-e2e:${PATH}"
  if [[ "$(id -u)" == 0 ]]; then
    as_root "${E2E_USER:-dev}"
  fi
  as_user
}

main "$@"
