"""`--retired-prefix` keeps citations of deliberately deleted code as history.

The Rust reference tree was deleted while ~110 notes still cite its tests as
the evidence they were written against. Those citations must stay readable but
must not fail verification; any other missing target still must.

Run: `python -X utf8 -m unittest discover -s tools/regression-notes/tests`
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).resolve().parent.parent / "verify-notes.py"

NOTE = """# Agent Note: Fixture decision

Status: implemented

## Problem

Fixture problem with enough prose to satisfy the linter for this fixture.

## Decision

Fixture decision.

## Alternatives considered

Fixture alternatives.

## Consequences

Fixture consequences.

## Verification

- `tests/test_login.py`
- `{cited}`

Proved: fixture proof line long enough to pass the linter checks here.
"""


class TestRetiredPrefix(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "tests").mkdir()
        (self.root / "tests" / "test_login.py").write_text("def test_login():\n    pass\n", encoding="utf-8")
        self.notes = self.root / "notes"
        self.note = self.notes / "implemented" / "bug-fix" / "2026-09-01-fixture-decision.md"
        self.note.parent.mkdir(parents=True)

    def tearDown(self):
        self._tmp.cleanup()

    def run_tool(self, cited, *extra):
        self.note.write_text(NOTE.format(cited=cited), encoding="utf-8")
        return subprocess.run(
            [sys.executable, str(TOOL), "--notes-dir", str(self.notes),
             "--repo-root", str(self.root), "--no-name-heuristic", *extra],
            capture_output=True, text=True)

    def test_retired_citation_is_history_not_an_error(self):
        cited = "src/core/cleaner.rs::old_rust_test"
        red = self.run_tool(cited)
        self.assertNotEqual(red.returncode, 0, "missing target must fail without the flag")
        green = self.run_tool(cited, "--retired-prefix", "src/")
        self.assertEqual(green.returncode, 0, green.stdout + green.stderr)

    def test_other_missing_targets_still_fail(self):
        result = self.run_tool("internal/core/gone_test.go::TestGone", "--retired-prefix", "src/")
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("gone_test.go", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
