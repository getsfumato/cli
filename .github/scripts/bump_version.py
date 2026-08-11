#!/usr/bin/env python3
"""Set the workspace version, in the four places that must agree.

`[workspace.package] version` is the one a human thinks of. The three
`[workspace.dependencies]` entries carry the same version beside their `path`,
which is what makes the crates publishable, and Cargo has no interpolation to keep
them in step — so a bump that misses one publishes a crate depending on a version
that never existed. `check_versions.py` refuses that, and this is what stops it
happening.

Rewrites only the version strings, so comments and layout survive: this file is
mostly load-bearing commentary.
"""

from __future__ import annotations

import re
import sys

MANIFEST = "Cargo.toml"
INTERNAL = ("sfumato-domain", "sfumato-core", "sfumato-adapters")
SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def bumped(text: str, version: str) -> str:
    """Return `text` with every workspace version set to `version`."""
    out, changed = re.subn(
        r'(?m)^(version = ")[^"]+(")', rf"\g<1>{version}\g<2>", text, count=1
    )
    if changed != 1:
        raise SystemExit("::error::found no `version = ` line to bump in Cargo.toml")

    for crate in INTERNAL:
        out, changed = re.subn(
            rf'(?m)^({re.escape(crate)} = \{{ version = ")[^"]+(")',
            rf"\g<1>{version}\g<2>",
            out,
            count=1,
        )
        if changed != 1:
            raise SystemExit(
                f"::error::found no [workspace.dependencies] entry for {crate}; "
                "it must carry a version beside its path to be publishable"
            )
    return out


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: bump_version.py <version>")
    version = sys.argv[1]
    if not SEMVER.match(version):
        raise SystemExit(f"::error::not a version: {version}")

    with open(MANIFEST) as handle:
        text = handle.read()
    with open(MANIFEST, "w") as handle:
        handle.write(bumped(text, version))
    print(f"set the workspace version to {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
