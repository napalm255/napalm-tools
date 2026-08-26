#!/usr/bin/env bash
# Remove everything this repository created on the host, and nothing else:
# build output, generated completions, and the container images the
# devcontainer and end-to-end runs pulled or built. Other images and
# containers on the machine are never touched. Nothing under $HOME - mise
# caches, Homebrew - is touched either: that belongs to the machine, not to
# this checkout.
#
#   scripts/clean.sh            remove
#   scripts/clean.sh --dry-run  say what would be removed
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dry=0
[ "${1:-}" = "--dry-run" ] && dry=1

say() { printf '%s\n' "$*"; }
run() {
  if [ "$dry" = 1 ]; then say "would: $*"; else say "$*"; "$@"; fi
}

# The images are named where they are used, so read them from there rather
# than keeping a second list that drifts.
images=(localhost/napalm-tools-dev)
for f in "$root/.devcontainer/Containerfile" "$root/tests/e2e/run.sh"; do
  while read -r digest; do
    case "$f" in
      *Containerfile) images+=("registry.fedoraproject.org/fedora@$digest") ;;
      *run.sh)        images+=("ghcr.io/ublue-os/bluefin-dx@$digest") ;;
    esac
  done < <(grep -o 'sha256:[0-9a-f]\{64\}' "$f")
done

if command -v podman >/dev/null; then
  for image in "${images[@]}"; do
    if ! podman image exists "$image"; then
      say "absent: $image"
      continue
    fi
    # Containers left from an interrupted run; e2e uses --rm so normally none.
    mapfile -t containers < <(podman ps -a -q --filter "ancestor=$image")
    for c in "${containers[@]}"; do
      run podman rm -f "$c"
    done
    run podman rmi "$image"
  done
else
  say "podman not installed; skipping images"
fi

if [ -d "$root/target" ]; then
  run cargo clean --manifest-path "$root/Cargo.toml"
else
  say "absent: target/"
fi
if [ -d "$root/completions" ]; then
  run rm -rf "$root/completions"
else
  say "absent: completions/"
fi
