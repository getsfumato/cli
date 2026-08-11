#!/usr/bin/env python3
"""Tests for release_plan.

This decides, unattended, what version reaches users and what the changelog says
about it. A wrong answer is a published version number that cannot be reused, so
the mapping is asserted rather than assumed.

Run: python3 .github/scripts/release_plan_test.py
"""

from __future__ import annotations

import sys
import unittest

sys.path.insert(0, __file__.rsplit("/", 1)[0])

from release_plan import Commit, plan  # noqa: E402

DATE = "2026-08-11"


def bump(messages: list[str], current: str = "0.3.0") -> str:
    return plan(messages, current, DATE)["bump"]


class Parsing(unittest.TestCase):
    def test_reads_type_scope_and_subject(self) -> None:
        commit = Commit("feat(browser): find a browser anywhere")
        self.assertTrue(commit.conventional)
        self.assertEqual(commit.type, "feat")
        self.assertEqual(commit.scope, "browser")
        self.assertEqual(commit.subject, "find a browser anywhere")
        self.assertFalse(commit.breaking)

    def test_a_scope_is_optional(self) -> None:
        commit = Commit("fix: stop segfaulting on musl")
        self.assertTrue(commit.conventional)
        self.assertEqual(commit.scope, "")

    def test_marks_a_bang_as_breaking(self) -> None:
        self.assertTrue(Commit("feat(cli)!: rename generate deck").breaking)

    def test_marks_a_breaking_change_footer_as_breaking(self) -> None:
        commit = Commit("feat(config): move browser_path\n\nBREAKING CHANGE: schema v6.")
        self.assertTrue(commit.breaking)

    def test_accepts_the_hyphenated_footer(self) -> None:
        self.assertTrue(Commit("fix: x\n\nBREAKING-CHANGE: y").breaking)

    def test_a_body_mentioning_breaking_change_mid_sentence_is_not_breaking(self) -> None:
        # Only a footer counts. Prose about a breaking change must not silently
        # bump the version.
        commit = Commit("docs: explain what a BREAKING CHANGE: footer does")
        self.assertFalse(commit.breaking)

    def test_flags_a_message_that_is_not_conventional(self) -> None:
        commit = Commit("just fixing stuff")
        self.assertFalse(commit.conventional)
        self.assertEqual(commit.subject, "just fixing stuff")


class Bumping(unittest.TestCase):
    def test_nothing_releasable_means_no_release(self) -> None:
        self.assertEqual(bump(["docs: tidy", "chore: bump deps", "ci: cache"]), "none")

    def test_a_fix_takes_the_patch(self) -> None:
        self.assertEqual(bump(["fix: a thing"]), "patch")

    def test_perf_takes_the_patch(self) -> None:
        self.assertEqual(bump(["perf: faster"]), "patch")

    def test_a_feature_takes_the_minor(self) -> None:
        self.assertEqual(bump(["feat: a thing"]), "minor")

    def test_the_largest_bump_wins(self) -> None:
        self.assertEqual(bump(["fix: a", "feat: b", "docs: c"]), "minor")
        self.assertEqual(bump(["docs: a", "fix: b"]), "patch")

    def test_below_one_zero_a_break_takes_the_minor(self) -> None:
        self.assertEqual(bump(["feat!: a break"], current="0.3.0"), "minor")

    def test_from_one_zero_a_break_takes_the_major(self) -> None:
        self.assertEqual(bump(["feat!: a break"], current="1.4.2"), "major")

    def test_an_unconventional_commit_alone_releases_nothing(self) -> None:
        self.assertEqual(bump(["whatever I felt like writing"]), "none")


class Versions(unittest.TestCase):
    def test_patch(self) -> None:
        self.assertEqual(plan(["fix: a"], "0.3.0", DATE)["version"], "0.3.1")

    def test_minor_resets_the_patch(self) -> None:
        self.assertEqual(plan(["feat: a"], "0.3.7", DATE)["version"], "0.4.0")

    def test_major_resets_minor_and_patch(self) -> None:
        self.assertEqual(plan(["feat!: a"], "1.4.7", DATE)["version"], "2.0.0")

    def test_no_release_keeps_the_version(self) -> None:
        result = plan(["docs: a"], "0.3.0", DATE)
        self.assertEqual(result["version"], "0.3.0")
        self.assertFalse(result["release"])
        self.assertEqual(result["notes"], "")


class Notes(unittest.TestCase):
    def test_groups_by_section_in_reading_order(self) -> None:
        notes = plan(
            [
                "docs: explain it",
                "fix(browser): find one on PATH",
                "feat(video): add a workflow flag",
                "feat(config)!: move browser_path",
            ],
            "0.3.0",
            DATE,
        )["notes"]

        self.assertIn("## 0.4.0 — 2026-08-11", notes)
        self.assertLess(notes.index("### Breaking"), notes.index("### Added"))
        self.assertLess(notes.index("### Added"), notes.index("### Fixed"))
        self.assertLess(notes.index("### Fixed"), notes.index("### Documentation"))

    def test_a_breaking_commit_appears_only_under_breaking(self) -> None:
        notes = plan(["feat(config)!: move browser_path"], "0.3.0", DATE)["notes"]
        self.assertIn("### Breaking", notes)
        self.assertNotIn("### Added", notes)

    def test_renders_the_scope_when_there_is_one(self) -> None:
        notes = plan(["fix(browser): find one"], "0.3.0", DATE)["notes"]
        self.assertIn("- **browser:** find one", notes)

    def test_renders_without_a_scope(self) -> None:
        notes = plan(["fix: find one"], "0.3.0", DATE)["notes"]
        self.assertIn("- find one", notes)

    def test_omits_noise_from_the_notes(self) -> None:
        # These can ride along in a release but should not be in what a reader is
        # handed.
        notes = plan(["feat: real change", "chore: deps", "ci: cache", "test: add"], "0.3.0", DATE)["notes"]
        self.assertNotIn("deps", notes)
        self.assertNotIn("cache", notes)


class Reporting(unittest.TestCase):
    def test_reports_a_type_nobody_recognises(self) -> None:
        # `feet` parses as a well-formed header and releases nothing, so it is
        # invisible unless it is called out. Silently ignoring it would hide the
        # typo from whoever expected a release.
        result = plan(["feet: a thing", "fix: a real one"], "0.3.0", DATE)
        self.assertEqual(result["unrecognised"], ["feet: a thing"])
        self.assertEqual(result["bump"], "patch")

    def test_reports_a_message_that_is_not_conventional_at_all(self) -> None:
        result = plan(["just fixing stuff", "fix: a real one"], "0.3.0", DATE)
        self.assertEqual(result["unrecognised"], ["just fixing stuff"])

    def test_says_nothing_about_deliberately_quiet_types(self) -> None:
        # chore/ci/docs release nothing by design; reporting them would make the
        # signal useless.
        result = plan(["chore: deps", "ci: cache", "docs: tidy", "fix: real"], "0.3.0", DATE)
        self.assertEqual(result["unrecognised"], [])

    def test_counts_what_it_considered(self) -> None:
        self.assertEqual(plan(["fix: a", "docs: b", ""], "0.3.0", DATE)["considered"], 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
