#!/usr/bin/env python3
"""Publication guard rejects plausible stale or ambiguous release text."""
from pathlib import Path
import runpy
import unittest

validate = runpy.run_path(str(Path(__file__).resolve().parents[3] / "scripts/release-notes.py"))["validate"]


class Notes(unittest.TestCase):
    def test_current_notes_may_reference_prior_versions_later(self):
        validate("RUWAC v0.4.51 improves latency.\nEarlier v0.4.50 added selection.\n",
                 "## Unreleased\n\n## v0.4.51 — release\n- change\n", "v0.4.51")

    def test_stale_body_cannot_pass_by_mentioning_target_later(self):
        with self.assertRaises(ValueError):
            validate("RUWAC v0.4.43\nUpgrade from v0.4.51\n", "## v0.4.51\n", "v0.4.51")

    def test_partial_versions_empty_or_missing_identity_are_rejected(self):
        for notes in ["", "RUWAC v0.4.510\n", "RUWAC v0.4.51-old\n", "Release changes\n"]:
            with self.subTest(notes=notes), self.assertRaises(ValueError):
                validate(notes, "## v0.4.51\n", "v0.4.51")

    def test_missing_or_duplicate_changelog_section_is_rejected(self):
        for changelog in ["## v0.4.50\n", "## v0.4.51\n\n## v0.4.51 — duplicate\n"]:
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                validate("RUWAC v0.4.51\n", changelog, "v0.4.51")


if __name__ == "__main__":
    unittest.main()
