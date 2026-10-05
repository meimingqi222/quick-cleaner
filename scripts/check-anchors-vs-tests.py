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

Exit 1 when any bound anchor is missing from every list.
"""
import sys
from pathlib import Path


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    if len(argv) < 2:
        print(__doc__.strip())
        return 2
    anchors_file, list_files = Path(argv[0]), argv[1:]

    anchors = []
    for line in anchors_file.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if line and "::" in line:
            anchors.append(line.rsplit("::", 1)[-1].strip())

    tests = set()
    for f in list_files:
        for line in Path(f).read_text(encoding="utf-8-sig").splitlines():
            line = line.strip()
            if line.endswith(": test"):
                tests.add(line[: -len(": test")].rsplit("::", 1)[-1])

    missing = [a for a in anchors if a not in tests]
    for a in missing:
        print(f"ERROR: bound anchor `{a}` is not a test in any listed binary")
    print(f"{len(anchors)} anchor(s) checked against {len(tests)} listed tests, "
          f"{len(missing)} missing")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
