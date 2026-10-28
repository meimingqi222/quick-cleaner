"""Regression tests for scripts/check-anchors-vs-tests.py.

The checker unions cargo test lists, but Python and Go tests never appear in
those lists. Anchors into those files are checked against the declaration in
the cited file instead. Go anchors were the second language to hit this gap:
every one of them was reported missing even though the tests existed, so the
behaviour is pinned here.

Run: `python -X utf8 -m unittest discover -s tools/regression-notes/tests`
"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

CHECKER = Path(__file__).resolve().parents[3] / "scripts" / "check-anchors-vs-tests.py"
SPEC = importlib.util.spec_from_file_location("check_anchors_vs_tests", CHECKER)
CHECKER_MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER_MODULE)

RUST_LIST = "core::apps::command_tests::follow_up_does_not_reopen_dialog: test\n"


def write(root, rel, content):
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return path


class AnchorCheckCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.anchors = self.root / "anchors.txt"
        self.tests = write(self.root, "testlist.txt", RUST_LIST)

    def tearDown(self):
        self._tmp.cleanup()

    def run_checker(self, anchors):
        self.anchors.write_text("\n".join(anchors) + "\n", encoding="utf-8")
        return CHECKER_MODULE.main(
            [str(self.anchors), str(self.tests)], root=self.root)


class TestFileDeclaredAnchors(AnchorCheckCase):
    def test_go_anchor_must_name_a_declared_function(self):
        write(self.root, "internal/core/identity_test.go",
              "package core\n\nfunc TestIdentityInPlaceWrite(t *testing.T) {}\n")
        self.assertEqual(
            self.run_checker(
                ["internal/core/identity_test.go::TestIdentityInPlaceWrite"]),
            0)

    def test_go_anchor_named_only_in_a_comment_is_missing(self):
        write(self.root, "internal/core/identity_test.go",
              "package core\n\n"
              "// func TestIdentityInPlaceWrite(t *testing.T) {}\n"
              "func TestIdentityInPlaceWriteRenamed(t *testing.T) {}\n")
        self.assertEqual(
            self.run_checker(
                ["internal/core/identity_test.go::TestIdentityInPlaceWrite"]),
            1)

    def test_go_anchor_into_a_missing_file_is_missing(self):
        self.assertEqual(
            self.run_checker(["internal/core/gone_test.go::TestGone"]), 1)

    def test_python_anchor_still_names_a_def(self):
        write(self.root, "scripts/test_demo.py", "def test_alpha():\n    pass\n")
        self.assertEqual(
            self.run_checker(["scripts/test_demo.py::test_alpha"]), 0)
        self.assertEqual(
            self.run_checker(["scripts/test_demo.py::test_missing"]), 1)


class TestListedAnchors(AnchorCheckCase):
    def test_rust_anchor_still_needs_a_listed_test(self):
        self.assertEqual(self.run_checker(["src/core/foo.rs::foo_test"]), 1)
        write(self.root, "listed.txt", RUST_LIST + "core::foo::foo_test: test\n")
        self.anchors.write_text("src/core/foo.rs::foo_test\n", encoding="utf-8")
        self.assertEqual(
            CHECKER_MODULE.main(
                [str(self.anchors), str(self.root / "listed.txt")],
                root=self.root),
            0)


if __name__ == "__main__":
    unittest.main()
