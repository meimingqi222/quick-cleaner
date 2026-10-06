"""Regression tests for the vendored regression-notes tool.

The full suite lives with the skill that owns the tool; these three tests pin the
behaviour this repository depends on and history has already regressed once:

- the archived-note seal is stable across checkouts (LF vs CRLF),
- `--seal` stays append-only while `--reseal` is the explicit migration path,
- `--dump-anchors` skips archived notes (sealed and frozen, so they cannot follow
  a renamed or retired test).

Run: `python -X utf8 -m unittest discover -s tools/regression-notes/tests`
(needs no packaging; paths resolve from this file).
"""

import subprocess
import sys
import tempfile
import unittest
from datetime import date
from pathlib import Path

TOOL = Path(__file__).resolve().parent.parent / "verify-notes.py"

NOTE = """# Agent Note: Fixture decision

Status: implemented
Archived: {archived}

## Problem

Fixture problem with enough prose to satisfy the linter for this fixture.

## Decision

Fixture decision.

## Alternatives considered

Fixture alternatives.

## Consequences

Fixture consequences.

## Verification

Covered by `tests/test_login.py`.

Proved: fixture proof line long enough to pass the linter checks here.
"""


def write_tree(root, files):
    for rel, content in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


class TestSealDigest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        (self.root / "tests").mkdir()
        (self.root / "tests" / "test_login.py").write_text("x", encoding="utf-8")
        self.notes = self.root / "notes"
        self.note_rel = "archived/bug-fix/2026-09-01-fixture-decision.md"
        self.note = self.notes / self.note_rel
        write_tree(self.notes, {self.note_rel: NOTE.format(archived=date.today())})

    def tearDown(self):
        self._tmp.cleanup()

    def run_tool(self, *extra):
        return subprocess.run(
            [sys.executable, str(TOOL), "--notes-dir", str(self.notes),
             "--repo-root", str(self.root), *extra],
            capture_output=True, text=True)

    def test_digest_ignores_line_endings(self):
        # Write the bytes explicitly: Path.write_text translates newlines on Windows,
        # which would make both halves of this test CRLF and hide the defect.
        lf = self.note.read_bytes().replace(b"\r\n", b"\n")
        self.note.write_bytes(lf.replace(b"\n", b"\r\n"))
        sealed = self.run_tool("--seal")
        self.assertEqual(sealed.returncode, 0, sealed.stdout + sealed.stderr)

        self.note.write_bytes(lf)
        verified = self.run_tool()
        self.assertEqual(verified.returncode, 0, verified.stdout + verified.stderr)

    def test_seal_never_rewrites_but_reseal_migrates(self):
        manifest = self.notes / "archived" / "manifest.json"
        self.assertEqual(self.run_tool("--seal").returncode, 0)
        before = manifest.read_bytes()

        self.note.write_text(
            self.note.read_text(encoding="utf-8") + "\nTampered.\n", encoding="utf-8")
        still_red = self.run_tool("--seal")
        self.assertNotEqual(still_red.returncode, 0, still_red.stdout)
        self.assertEqual(manifest.read_bytes(), before, "seal must stay append-only")

        migrated = self.run_tool("--reseal")
        self.assertEqual(migrated.returncode, 0, migrated.stdout + migrated.stderr)
        self.assertIn(f"resealed {self.note_rel}", migrated.stdout)
        self.assertNotEqual(manifest.read_bytes(), before)

    def test_dump_anchors_skips_archived_notes(self):
        body = self.note.read_text(encoding="utf-8")
        self.note.write_text(
            body.replace("`tests/test_login.py`", "`tests/test_login.py::test_retired`"),
            encoding="utf-8")
        dumped = self.run_tool("--dump-anchors")
        self.assertEqual(dumped.returncode, 0, dumped.stdout + dumped.stderr)
        self.assertNotIn("test_retired", dumped.stdout)


if __name__ == "__main__":
    unittest.main()
