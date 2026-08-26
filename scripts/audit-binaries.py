#!/usr/bin/env python3
"""Check that each catalog package declares the binary it actually installs.

A wrong binary name is silent: the package installs, but the presence check
never matches it, so `nt` reinstalls it whenever the owning manager happens not
to report it. This compares declarations against the machine's real state and
reports any package a manager calls installed whose declared binary is absent.

Only packages currently installed can be checked; the rest are skipped.
"""

import re
import shutil
import subprocess
import sys
from pathlib import Path

CATALOG = Path(__file__).resolve().parent.parent / "src/bundles/catalog.rs"

PKG = re.compile(
    r'Pkg \{\s*name:\s*"([^"]+)",\s*binary:\s*(?:None|Some\("([^"]+)"\)),\s*'
    r"providers:\s*&\[Provider::\w+\(([^)]*)\)\]",
    re.S,
)


def brew_list(*args: str) -> set[str]:
    out = subprocess.run(
        ["brew", "list", *args], capture_output=True, text=True, check=False
    )
    return set(out.stdout.split())


def main() -> int:
    if not shutil.which("brew"):
        print("brew not available; nothing to check", file=sys.stderr)
        return 0

    formulae = brew_list("--formula", "-1")
    casks = brew_list("--cask")
    src = CATALOG.read_text()

    checked = 0
    suspects = []
    for name, binary, args in PKG.findall(src):
        if not binary:
            continue
        checked += 1
        ids = re.findall(r'"([^"]+)"', args)
        pkg_id = ids[0] if ids else name
        if "BrewCask" in args:
            installed = pkg_id in casks
        elif "ManagerId::Brew," in args:
            installed = pkg_id in formulae
        else:
            continue
        if installed and shutil.which(binary) is None:
            suspects.append((name, binary, pkg_id))

    print(f"checked {checked} packages with a declared binary")
    if not suspects:
        print("all declared binaries resolve")
        return 0

    print("\nmanager reports installed, but the declared binary is absent:")
    for name, binary, pkg_id in suspects:
        print(f"  {name:<20} declares {binary!r} (installed as {pkg_id})")
    return 1


if __name__ == "__main__":
    sys.exit(main())
