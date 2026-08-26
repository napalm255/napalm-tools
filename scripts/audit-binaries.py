#!/usr/bin/env python3
"""Check that each catalog package declares the binary it actually installs.

A wrong binary name is silent: the package installs, but the presence check
never matches it, so `nt` reinstalls it whenever the owning manager happens not
to report it. This asks the built `nt` for its catalog, compares each
declaration against the machine's real state, and reports any package a
manager calls installed whose declared binary is absent.

Only packages currently installed can be checked; the rest are skipped.
"""

import json
import shutil
import subprocess
import sys
from pathlib import Path

NT = Path(__file__).resolve().parent.parent / "target/release/nt"


def brew_list(*args: str) -> set[str]:
    out = subprocess.run(
        ["brew", "list", *args], capture_output=True, text=True, check=False
    )
    return set(out.stdout.split())


def main() -> int:
    if not NT.exists():
        print("build first: cargo build --release", file=sys.stderr)
        return 1
    if not shutil.which("brew"):
        print("brew not available; nothing to check", file=sys.stderr)
        return 0

    catalog = json.loads(
        subprocess.run(
            [str(NT), "bundles", "--output", "json"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
    )
    formulae = brew_list("--formula", "-1")
    casks = brew_list("--cask")

    checked = 0
    suspects = []
    for bundle in catalog["bundles"]:
        for pkg in bundle["packages"]:
            binary = pkg["binary"]
            if not binary:
                continue
            provider = pkg["providers"][0]
            if provider["manager"] == "brew":
                installed = provider["id"] in formulae
            elif provider["manager"] == "brew-cask":
                installed = provider["id"] in casks
            else:
                continue
            checked += 1
            if installed and shutil.which(binary) is None:
                suspects.append((bundle["name"], pkg["name"], binary, provider["id"]))

    print(f"checked {checked} Homebrew packages with a declared binary")
    for bundle, name, binary, pkg_id in suspects:
        print(f"  {bundle}/{name}: {pkg_id} is installed but `{binary}` is not on PATH")
    return 1 if suspects else 0


if __name__ == "__main__":
    sys.exit(main())
