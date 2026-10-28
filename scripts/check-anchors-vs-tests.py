"""Check that every `path::anchor` bound in notes is a real runnable test.

verify-notes.py checks an anchor is a substring of the cited file; a renamed
test survives as an orphan substring in a comment. This script takes the anchor
list (`--dump-anchors` output) and one or more test-binary listings (`cargo
test -- --list`, lines like `path::to::test_name: test`) and fails when an
anchor names no listed test.

`#[cfg]`-gated tests only exist in their platform's binary, so pass the
--list output from *every* platform job — the lists are unioned, and an anchor
is only "missing" when no platform lists it.

Usage:
    python tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --dump-anchors > anchors.txt
    python scripts/check-anchors-vs-tests.py anchors.txt testlist-windows.txt testlist-macos.txt

Anchors in Python and Go test files (`scripts/test_*.py::name`,
`internal/**/*_test.go::TestName`) are not in any cargo list; they must
instead name a `def`/`func` declaration in the cited file, which each
language's own workflow runs.

Exit 1 when any bound anchor is missing from every list.
"""
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
    argv = argv if argv is not None else sys.argv[1:]
    if len(argv) < 2:
        print(__doc__.strip())
        return 2
    anchors_file, list_files = Path(argv[0]), argv[1:]

    anchors = []
    file_missing = []
    for line in anchors_file.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if not line or "::" not in line:
            continue
        path, name = (part.strip() for part in line.rsplit("::", 1))
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
