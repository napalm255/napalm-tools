#!/usr/bin/env bash
# Remove everything this repository created on the host, and nothing else:
# build output, generated completions, and the container images the
# devcontainer and end-to-end runs pulled or built. Other images and
# containers on the machine are never touched. Nothing under $HOME - mise
# caches, Homebrew - is touched either: that belongs to the machine, not to
# this checkout.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: scripts/clean.sh [--dry-run]

  -n, --dry-run   say what would be removed, remove nothing
  -h, --help      show this help
USAGE
}

dry=0
failed=()

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }

# Run a removal, or say what it would be. A failure is recorded rather than
# fatal, so one stubborn image never leaves the rest of the tree uncleaned.
run() {
  if [[ ${dry} == 1 ]]; then
    say "would: $*"
    return 0
  fi
  say "$*"
  "$@" || failed+=("$*")
}

# The images are named where they are used - the e2e driver and the
# Containerfile - and read from there rather than kept in a second list.
images() {
  local run_sh="$1" containerfile="$2" digest name
  name="$(grep -o 'image="localhost/[^"]*"' "${run_sh}" | cut -d'"' -f2 || true)"
  if [[ -z ${name} ]]; then
    warn "no local image name found in ${run_sh}; the dev image will not be cleaned"
  else
    printf '%s\n' "${name}"
  fi
  while read -r digest; do
    printf 'registry.fedoraproject.org/fedora@%s\n' "${digest}"
  done < <(grep -o 'sha256:[0-9a-f]\{64\}' "${containerfile}" || true)
  while read -r digest; do
    printf 'ghcr.io/ublue-os/bluefin-dx@%s\n' "${digest}"
  done < <(grep -o 'sha256:[0-9a-f]\{64\}' "${run_sh}" || true)
}

clean_build_output() {
  local root="$1"
  if [[ -d "${root}/target" ]]; then
    if command -v cargo >/dev/null 2>&1; then
      run cargo clean --manifest-path "${root}/Cargo.toml"
    else
      run rm -rf "${root}/target"
    fi
  else
    say "absent: target/"
  fi
  if [[ -d "${root}/completions" ]]; then
    run rm -rf "${root}/completions"
  else
    say "absent: completions/"
  fi
}

clean_images() {
  local root="$1" image containers c
  if ! command -v podman >/dev/null 2>&1; then
    say "podman not installed; skipping images"
    return 0
  fi
  while read -r image; do
    [[ -n ${image} ]] || continue
    if ! podman image exists "${image}"; then
      say "absent: ${image}"
      continue
    fi
    # Containers left from an interrupted run; e2e uses --rm so normally none.
    mapfile -t containers < <(podman ps -a -q --filter "ancestor=${image}")
    for c in "${containers[@]}"; do
      run podman rm -f "${c}"
    done
    run podman rmi "${image}"
  done < <(images "${root}/tests/e2e/run.sh" "${root}/.devcontainer/Containerfile")
}

main() {
  local root
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -n | --dry-run) dry=1 ;;
      -h | --help)
        usage
        return 0
        ;;
      *)
        printf 'unknown argument: %s\n\n' "$1" >&2
        usage >&2
        return 2
        ;;
    esac
    shift
  done

  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  # Never build an rm -rf target from a root that is not this checkout.
  [[ -f "${root}/Cargo.toml" && -d "${root}/.git" ]] || {
    printf 'not a napalm-tools checkout: %s\n' "${root}" >&2
    return 1
  }

  # Cheap and local first, so a slow or failing image removal never leaves
  # the build output behind.
  clean_build_output "${root}"
  clean_images "${root}"

  if [[ ${#failed[@]} -gt 0 ]]; then
    printf '\n%d removal(s) failed:\n' "${#failed[@]}" >&2
    printf '  %s\n' "${failed[@]}" >&2
    return 1
  fi
}

main "$@"
