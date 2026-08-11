#!/usr/bin/env python3
"""Insert a release's notes at the top of CHANGELOG.md, below the preamble.

Idempotent: a section for the version already present is left alone rather than
duplicated, so a re-run of a partially failed release does not produce two.

usage: prepend_changelog.py <version> <plan.json>
"""

from __future__ import annotations

import json
import re
import sys

CHANGELOG = "CHANGELOG.md"


def inserted(existing: str, version: str, notes: str) -> str:
    if re.search(rf"(?m)^## {re.escape(version)}\b", existing):
        print(f"CHANGELOG.md already has a section for {version}, leaving it alone")
        return existing

    lines = existing.splitlines(keepends=True)
    # Below the preamble, above the newest existing release. Falls back to the end
    # of the preamble when there is no release section yet.
    for index, line in enumerate(lines):
        if line.startswith("## "):
            break
    else:
        index = len(lines)

    body = notes if notes.endswith("\n\n") else notes.rstrip("\n") + "\n\n"
    return "".join(lines[:index]) + body + "".join(lines[index:])


def main() -> int:
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepend_changelog.py <version> <plan.json>")
    version, plan_path = sys.argv[1], sys.argv[2]

    with open(plan_path) as handle:
        notes = json.load(handle)["notes"]
    if not notes.strip():
        raise SystemExit("::error::the plan carries no notes; refusing to write an empty section")

    with open(CHANGELOG) as handle:
        existing = handle.read()
    result = inserted(existing, version, notes)
    if result != existing:
        with open(CHANGELOG, "w") as handle:
            handle.write(result)
        print(f"wrote the {version} section")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
