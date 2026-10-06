"""Date-only note records must survive verification in a different timezone."""

import contextlib
import datetime
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOL = Path(__file__).resolve().parent.parent / "verify-notes.py"
SPEC = importlib.util.spec_from_file_location("note_date_verifier", TOOL)
VERIFIER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFIER)

BODY = """## Problem

A date-only record was written in another timezone.

## Decision

Validate the date without depending on the runner timezone.

## Alternatives considered

Dropping date validation would permit genuinely future records.

## Consequences

The same note is verifiable across machines.

## Verification

Covered by `tests/test_login.py`.

Proved: fixture proof sufficient to exercise the complete verifier.
"""


class TestDateTimezone(unittest.TestCase):
    def verify_at(self, instant, filename_date, archived_date=None):
        real_date, real_datetime = datetime.date, datetime.datetime

        class RunnerDate(real_date):
            @classmethod
            def today(cls):
                return instant.date()

        class FrozenDatetime(real_datetime):
            @classmethod
            def now(cls, tz=None):
                return instant.astimezone(tz) if tz else instant.replace(tzinfo=None)

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "tests").mkdir()
            (root / "tests/test_login.py").write_text("# fixture\n", encoding="utf-8")
            lifecycle = "archived" if archived_date else "implemented"
            note = root / "notes" / lifecycle / "bug-fix" / f"{filename_date}-fixture.md"
            note.parent.mkdir(parents=True)
            archive = f"Archived: {archived_date}\n" if archived_date else ""
            note.write_text(f"# Agent Note: Timezone fixture\n\nStatus: implemented\n{archive}\n{BODY}", encoding="utf-8")
            output = io.StringIO()
            with mock.patch.object(VERIFIER.datetime, "date", RunnerDate), \
                    mock.patch.object(VERIFIER.datetime, "datetime", FrozenDatetime), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                result = VERIFIER.main(["--notes-dir", str(root / "notes"), "--repo-root", str(root), "--seal"])
            return result, output.getvalue()

    def test_author_today_is_not_future_on_utc_runner(self):
        instant = datetime.datetime(2026, 10, 6, 21, 50, tzinfo=datetime.timezone.utc)
        for archived in [None, "2026-10-07"]:
            with self.subTest(archived=archived):
                code, output = self.verify_at(instant, "2026-10-07", archived)
                self.assertEqual(code, 0, output)

    def test_dates_beyond_today_in_every_timezone_are_rejected(self):
        instant = datetime.datetime(2026, 10, 6, 21, 50, tzinfo=datetime.timezone.utc)
        for filename, archive, message in [
            ("2026-10-08", None, "filename date cannot be in the future"),
            ("2026-10-06", "2026-10-08", "archived date cannot be in the future"),
        ]:
            code, output = self.verify_at(instant, filename, archive)
            self.assertNotEqual(code, 0)
            self.assertIn(message, output)

    def test_utc_morning_does_not_unconditionally_allow_tomorrow(self):
        instant = datetime.datetime(2026, 10, 6, 0, 0, tzinfo=datetime.timezone.utc)
        code, output = self.verify_at(instant, "2026-10-07")
        self.assertNotEqual(code, 0)
        self.assertIn("filename date cannot be in the future", output)

    def test_calendar_and_archive_order_remain_strict(self):
        instant = datetime.datetime(2026, 10, 6, 21, 50, tzinfo=datetime.timezone.utc)
        for filename, archive, message in [
            ("2026-02-30", None, "filename date is not a valid calendar date"),
            ("2026-10-07", "2026-10-06", "is before filename date"),
        ]:
            code, output = self.verify_at(instant, filename, archive)
            self.assertNotEqual(code, 0)
            self.assertIn(message, output)


if __name__ == "__main__":
    unittest.main()
