#!/usr/bin/env python3
"""Check that each catalog package declares the binary it actually installs.

A wrong binary name is silent: the package installs, but the presence check
never matches it, so `nt` reinstalls it whenever the owning manager happens not
to report it. This asks the built `nt` for its catalog, compares each
declaration against the machine's real state, and reports any package a
manager calls installed whose declared binary is absent.

Only Homebrew packages currently installed can be checked; the rest are
skipped. Binaries are resolved the way `nt` resolves them: on PATH and in the
directories the bootstrapped managers install into.
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

NT = Path(__file__).resolve().parent.parent / "target/release/nt"

# Mirrors `managers::known_tool_dirs` in src/managers/mod.rs.
TOOL_DIRS = [
    Path("/home/linuxbrew/.linuxbrew/bin"),
    Path.home() / ".local/bin",
    Path.home() / ".local/share/mise/shims",
]


def resolves(binary: str) -> bool:
    """Whether `nt` would find `binary`: on PATH or in a known tool directory."""
    if shutil.which(binary):
        return True
    return any(os.access(d / binary, os.X_OK) for d in TOOL_DIRS)


def run(cmd: list[str]) -> str:
    """Run a command and return its stdout, or fail loudly with its stderr."""
    try:
        return subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
    except subprocess.CalledProcessError as err:
        print(f"{' '.join(cmd)} failed ({err.returncode}):", file=sys.stderr)
        print(err.stderr.rstrip(), file=sys.stderr)
        raise SystemExit(1) from err


def main() -> int:
    if not NT.exists():
        print("build first: cargo build --release", file=sys.stderr)
        return 1
    if not shutil.which("brew"):
        print("brew not available; nothing to check", file=sys.stderr)
        return 0

    catalog = json.loads(run([str(NT), "bundles", "--output", "json"]))
    installed = {
        "brew": set(run(["brew", "list", "--formula", "-1"]).split()),
        "brew-cask": set(run(["brew", "list", "--cask", "-1"]).split()),
    }

    checked = 0
    suspects = []
    for bundle in catalog["bundles"]:
        for pkg in bundle["packages"]:
            binary = pkg["binary"]
            if not binary:
                continue
            # Every Homebrew provider is a candidate, not only the preferred
            # one: whichever of them is installed is the one to check.
            for provider in pkg["providers"]:
                owned = installed.get(provider["manager"])
                if owned is None or provider["id"] not in owned:
                    continue
                checked += 1
                if not resolves(binary):
                    suspects.append(
                        (bundle["name"], pkg["name"], binary, provider["id"])
                    )

    print(f"checked {checked} installed Homebrew packages with a declared binary")
    for bundle, name, binary, pkg_id in suspects:
        print(
            f"  {bundle}/{name}: {pkg_id} is installed but `{binary}` does not resolve"
        )
    return 1 if suspects else 0


if __name__ == "__main__":
    sys.exit(main())
