#!/usr/bin/env python3
"""Decide whether a set of commits warrants a release, and which one.

Reads Conventional Commits on stdin, separated by a NUL byte so that a commit
body containing blank lines cannot be mistaken for a boundary. Prints one JSON
object: the bump, the next version, and the changelog section for it.

Kept out of the workflow YAML so it can be tested. `release_plan_test.py` is the
test; CI runs it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys

# `type(optional scope)!: subject`. The `!` is one of the two ways to mark a
# break; `BREAKING CHANGE:` in the body is the other, and both are honoured.
HEADER = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?: (?P<subject>.+)$"
)
BREAKING_BODY = re.compile(r"^BREAKING[ -]CHANGE:", re.MULTILINE)

# Types that can cause a release, and what they cause on their own.
TRIGGERS = {"feat": "minor", "fix": "patch", "perf": "patch"}

# Types that are deliberately not releasable. Kept explicit so that a *typo* in a
# type is distinguishable from a type that means "no release": `feet: …` parses as
# a well-formed header, releases nothing, and would otherwise be indistinguishable
# from `chore: …`.
QUIET = {"docs", "style", "refactor", "test", "build", "ci", "chore", "revert"}
KNOWN = set(TRIGGERS) | QUIET

# Types worth telling a reader about, in the order a reader wants them, with the
# heading each becomes.
SECTIONS = [
    ("breaking", "Breaking"),
    ("feat", "Added"),
    ("fix", "Fixed"),
    ("perf", "Performance"),
    ("refactor", "Changed"),
    ("docs", "Documentation"),
]

PRECEDENCE = {"none": 0, "patch": 1, "minor": 2, "major": 3}


class Commit:
    """One parsed Conventional Commit."""

    def __init__(self, message: str) -> None:
        lines = message.strip().splitlines()
        header = lines[0] if lines else ""
        body = "\n".join(lines[1:])
        match = HEADER.match(header)
        self.conventional = match is not None
        self.type = match.group("type") if match else ""
        self.scope = (match.group("scope") or "") if match else ""
        self.subject = match.group("subject") if match else header
        self.breaking = bool(
            (match and match.group("breaking")) or BREAKING_BODY.search(body)
        )

    def section(self) -> str | None:
        """Which changelog section this belongs under, if any."""
        if self.breaking:
            return "breaking"
        return self.type if any(self.type == key for key, _ in SECTIONS) else None

    def recognised(self) -> bool:
        """Whether this will be understood rather than silently skipped."""
        return self.conventional and self.type in KNOWN

    def entry(self) -> str:
        return f"- **{self.scope}:** {self.subject}" if self.scope else f"- {self.subject}"


def bump_for(commits: list[Commit], major: int) -> str:
    """The largest bump the commits call for.

    Below 1.0 a break takes the minor rather than the major: there is no major to
    spend, and semver says anything may change in 0.x. It collapses breaks and
    features onto the same bump, which is a real loss of signal and the reason the
    changelog still separates them. Above 1.0 the mapping is the ordinary one.
    """
    result = "none"
    for commit in commits:
        if commit.breaking:
            candidate = "minor" if major == 0 else "major"
        else:
            candidate = TRIGGERS.get(commit.type, "none")
        if PRECEDENCE[candidate] > PRECEDENCE[result]:
            result = candidate
    return result


def next_version(current: str, bump: str) -> str:
    major, minor, patch = (int(part) for part in current.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    return current


def notes_for(commits: list[Commit], version: str, date: str) -> str:
    grouped: dict[str, list[str]] = {}
    for commit in commits:
        section = commit.section()
        if section:
            grouped.setdefault(section, []).append(commit.entry())

    lines = [f"## {version} — {date}", ""]
    for key, heading in SECTIONS:
        if key in grouped:
            lines.append(f"### {heading}")
            lines.append("")
            lines.extend(grouped[key])
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def plan(messages: list[str], current: str, date: str) -> dict:
    commits = [Commit(message) for message in messages if message.strip()]
    major = int(current.split(".")[0])
    bump = bump_for(commits, major)
    version = next_version(current, bump)
    return {
        "bump": bump,
        "release": bump != "none",
        "current": current,
        "version": version,
        "notes": notes_for(commits, version, date) if bump != "none" else "",
        "considered": len(commits),
        # Surfaced rather than silently dropped. Two ways a commit can be invisible
        # to the bump: it is not Conventional-Commit-shaped at all, or it is shaped
        # correctly around a type nobody recognises. A `feat` typo'd as `feet`
        # parses cleanly and releases nothing, so it needs saying just as loudly.
        "unrecognised": [
            (c.subject if not c.conventional else f"{c.type}: {c.subject}")
            for c in commits
            if not c.recognised()
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", required=True, help="version being released from")
    parser.add_argument("--date", required=True, help="release date, YYYY-MM-DD")
    args = parser.parse_args()

    messages = sys.stdin.read().split("\0")
    json.dump(plan(messages, args.current, args.date), sys.stdout, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
