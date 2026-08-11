#!/usr/bin/env python3
"""Check that every crate, and every dependency between them, agrees on a version.

Guards the one publishing mistake that cannot be undone. `cargo publish --dry-run`
resolves an internal dependency through its `path` and so never looks at the
`version` beside it: a crate can be published declaring a dependency on a version
of its sibling that never existed, and crates.io does not allow deleting or
reusing a version to fix it.

Reads `cargo metadata --no-deps --format-version 1` from a file (default
/tmp/meta.json). With TAG set in the environment, every crate must be at that
version too — that is the tag-matches-the-manifest check.
"""

from __future__ import annotations

import json
import os
import sys


def problems(meta: dict, expected: str | None) -> list[str]:
    versions = {p["name"]: p["version"] for p in meta["packages"]}
    found = []

    if expected is not None:
        wrong = {name: v for name, v in versions.items() if v != expected}
        if wrong:
            found.append(f"tag says {expected}, these do not: {wrong}")

    distinct = set(versions.values())
    if len(distinct) > 1:
        # The four move in lockstep. Divergence would mean real semver ranges
        # between layers that change together daily.
        found.append(f"crates are not in lockstep: {sorted(distinct)}")

    for package in meta["packages"]:
        for dep in package["dependencies"]:
            if not dep["name"].startswith("sfumato"):
                continue
            if dep["name"] not in versions:
                found.append(f'{package["name"]} depends on unknown {dep["name"]}')
                continue
            want = "^" + versions[dep["name"]]
            if dep["req"] != want:
                found.append(
                    f'{package["name"]} -> {dep["name"]} {dep["req"]} (want {want})'
                )
    return found


def main() -> int:
    path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/meta.json"
    expected = os.environ.get("TAG") or None
    with open(path) as handle:
        meta = json.load(handle)

    found = problems(meta, expected)
    if found:
        for problem in found:
            print(f"::error::{problem}")
        return 1

    versions = sorted({p["version"] for p in meta["packages"]})
    print(f"all crates and every internal requirement agree: {versions[0]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
