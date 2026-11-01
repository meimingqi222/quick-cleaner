"""Check that every `path::anchor` bound in notes is a real test.

verify-notes.py checks an anchor is a substring of the cited file; a renamed
test survives as an orphan substring in a comment. This script takes the anchor
list (`--dump-anchors` output) and fails when an anchor is not a real test.

Anchors in Go and Python test files (`internal/**/*_test.go::TestName`,
`scripts/test_*.py::name`) must name a `func`/`def` declaration in the cited
file. Anchors in any other language need optional test-binary listings (lines
like `path::to::test_name: test`, unioned across platforms); this was the Rust
`cargo test -- --list` path, kept generic after the Rust tree was removed.
`--dump-anchors` also emits anchors into deleted files; pass the deleted
directories with `--retired-prefix` so those historical citations are skipped.

Usage:
    python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --dump-anchors > anchors.txt
    python scripts/check-anchors-vs-tests.py anchors.txt [testlist ...] [--retired-prefix src/ ...]

Exit 1 when any bound anchor is missing.
"""
import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Anchors into a test file whose tests never reach a cargo list are checked
# against the declaration in that file: Go's `func TestX(`, Python's `def`.
FILE_DECLARATION = {
    ".py": r"^\s*def {name}\(",
    ".go": r"^func {name}\(",
}


def main(argv=None, root=ROOT):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("anchors", help="--dump-anchors output")
    ap.add_argument("lists", nargs="*", help="optional test-binary listings, unioned")
    # Anchors into deliberately deleted code are history, matching
    # verify-notes.py's option of the same name.
    ap.add_argument("--retired-prefix", action="append", default=[], metavar="PATH/")
    args = ap.parse_args(argv if argv is not None else sys.argv[1:])
    anchors_file, list_files, retired = Path(args.anchors), args.lists, args.retired_prefix

    anchors = []
    file_missing = []
    for line in anchors_file.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if not line or "::" not in line:
            continue
        path, name = (part.strip() for part in line.rsplit("::", 1))
        if path.replace("\\", "/").startswith(tuple(retired)):
            continue
        declaration = FILE_DECLARATION.get(Path(path).suffix)
        if declaration is not None:
            source = root / path
            text = source.read_text(encoding="utf-8") if source.is_file() else ""
            if not re.search(declaration.format(name=re.escape(name)), text, flags=re.M):
                file_missing.append(f"{path}::{name}")
            continue
        anchors.append(name)

    tests = set()
    for f in list_files:
        for line in Path(f).read_text(encoding="utf-8-sig").splitlines():
            line = line.strip()
            if line.endswith(": test"):
                tests.add(line[: -len(": test")].rsplit("::", 1)[-1])

    missing = [a for a in anchors if a not in tests]
    for a in missing:
        print(f"ERROR: bound anchor `{a}` is not a test in any listed binary")
    for a in file_missing:
        print(f"ERROR: bound anchor `{a}` names no test function in that file")
    print(f"{len(anchors)} anchor(s) checked against {len(tests)} listed tests, "
          f"{len(missing) + len(file_missing)} missing")
    return 1 if missing or file_missing else 0


if __name__ == "__main__":
    sys.exit(main())
