#!/usr/bin/env bash
# End-to-end: run the release `nt` for real inside a container, as an
# ordinary user with passwordless sudo, and check that it converges and that
# a second run is a no-op.
#
#   tests/e2e/run.sh fedora     # the devcontainer image (registry.fedoraproject.org/fedora)
#   tests/e2e/run.sh bluefin    # ghcr.io/ublue-os/bluefin-dx, treated as atomic
#
# E2E_BUNDLES limits what is applied (default: a fast, representative set).
# E2E_FULL=1 applies everything, which downloads several gigabytes.
set -euo pipefail

main() {
  local target="${1:?usage: run.sh fedora|bluefin}" here root bin image user extra_env
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  root="$(cd "${here}/../.." && pwd)"
  bin="${root}/target/release/nt"

  [[ -x ${bin} ]] || {
    echo "build first: cargo build --release" >&2
    exit 1
  }
  command -v podman >/dev/null || {
    echo "podman is required" >&2
    exit 1
  }

  case "${target}" in
    fedora)
      image="localhost/napalm-tools-dev"
      user=dev
      extra_env=()
      ;;
    bluefin)
      # Pinned by digest (bluefin-dx:stable, 44.20260825). Bump deliberately.
      image="ghcr.io/ublue-os/bluefin-dx@sha256:a97bdb9d3efe9c65769d36f887155973ba0d9675e3d1d12aaaeaa6f15cd66429"
      user=dev
      # A container is not ostree-booted, but the image is; tell nt so.
      extra_env=(-e NT_OSTREE_MARKER=/etc/os-release)
      ;;
    *)
      echo "unknown target: ${target}" >&2
      exit 1
      ;;
  esac

  # Say what is happening before the slow part: building the Fedora image
  # can take minutes and is otherwise silent.
  echo "== e2e: ${target} (${image})"
  if [[ ${target} == fedora ]]; then
    echo "== building ${image}"
    podman build -q -t "${image}" -f "${root}/.devcontainer/Containerfile" "${root}" >/dev/null
  fi

  # Under /var/tmp, not /usr/local: on an ostree image /usr/local is a symlink
  # into /var that does not exist inside a container. The mounts are `:z`
  # (shared label), not `:Z`: inside.sh is a tracked file and the binary is
  # build output, and neither should be relabelled as private to one container.
  podman run --rm \
    -v "${bin}:/var/tmp/nt-e2e/nt:ro,z" \
    -v "${here}/inside.sh:/var/tmp/nt-e2e/inside.sh:ro,z" \
    -e "E2E_USER=${user}" \
    -e "E2E_BUNDLES=${E2E_BUNDLES:-core shell prompt go rust}" \
    -e "E2E_FULL=${E2E_FULL:-0}" \
    "${extra_env[@]}" \
    "${image}" bash /var/tmp/nt-e2e/inside.sh
}

main "$@"
