"""Check that every `Note:` pointer in docs/PITFALLS.md resolves to a live note.

A `Note:` line under a `## P<n>` heading names note basenames. The pointer must
resolve to a note under docs/agent-notes/, and it must be *live* authority —
implemented, not archived. An archived or deleted target means the pitfall's
checklist still points at the right idea but the locking tests may have moved;
the pointer must be updated in the same change that archives the note.

Warnings (never errors): a `## P<n>` section with no `Note:` line, or a
`Note:`-cited note that does not cite the pitfall's test names is not checked
here — this script only verifies pointer integrity.

Usage: python scripts/check-pitfall-notes.py [--pitfalls docs/PITFALLS.md]
       [--notes-dir docs/agent-notes]
Exit 1 on any error.
"""
import argparse
import re
import sys
from pathlib import Path

HEADING_RE = re.compile(r"^## (P\d+)\b")
NOTE_LINE_RE = re.compile(r"^Note:\s*(.+)$")
NOTE_NAME_RE = re.compile(r"`?(\d{4}-\d{2}-\d{2}-[A-Za-z0-9._-]+\.md)`?")

BODILESS = {"proposed", "rejected", "archived"}


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--pitfalls", default="docs/PITFALLS.md")
    ap.add_argument("--notes-dir", default="docs/agent-notes")
    args = ap.parse_args(argv)

    pitfalls = Path(args.pitfalls)
    notes_root = Path(args.notes_dir)

    # Map basename -> lifecycle folder for every note in the tree.
    note_by_name = {}
    if notes_root.is_dir():
        for md in notes_root.rglob("*.md"):
            if md.name.endswith(".zh.md"):
                continue
            try:
                rel_parts = md.relative_to(notes_root).parts
            except ValueError:
                continue
            note_by_name[md.name] = rel_parts[0] if len(rel_parts) > 1 else ""

    errors, warnings = [], []
    current, current_line = None, 0
    section_has_note = {}
    for i, line in enumerate(pitfalls.read_text(encoding="utf-8-sig").splitlines(), 1):
        m = HEADING_RE.match(line)
        if m:
            current, current_line = m.group(1), i
            section_has_note.setdefault(current, False)
            continue
        if current is None:
            continue
        n = NOTE_LINE_RE.match(line)
        if not n:
            continue
        section_has_note[current] = True
        for name in NOTE_NAME_RE.findall(n.group(1)):
            lifecycle = note_by_name.get(name)
            if lifecycle is None:
                errors.append(f"{pitfalls}:{i}: `Note:` cites {name}, which is not in {notes_root}")
            elif lifecycle != "implemented":
                errors.append(
                    f"{pitfalls}:{i}: `Note:` cites {name}, which lives under "
                    f"{lifecycle}/ — a pitfall must point at live authority; "
                    f"update the pointer in the same change that archives it")

    for pid, has in section_has_note.items():
        if not has:
            warnings.append(f"WARNING: {pitfalls}: {pid} has no `Note:` pointer")

    for w in warnings:
        print(w)
    for e in errors:
        print(f"ERROR: {e}")
    print(f"{len(errors)} error(s), {len(warnings)} warning(s), "
          f"{sum(1 for v in section_has_note.values() if v)} of "
          f"{len(section_has_note)} pitfall(s) carry Note: pointers")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
