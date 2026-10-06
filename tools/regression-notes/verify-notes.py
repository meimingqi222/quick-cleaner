#!/usr/bin/env python3
"""Verify regression-notes tree: layout, format, status consistency, test binding.

Usage:
    python3 verify-notes.py [--notes-dir .agents/notes] [--repo-root .]
                            [--no-strict] [--allow-missing] [--seal]
                            [--strict-anchors] [--no-name-heuristic]
                            [--no-bare-resolution] [--find REGEX]
                            [--for-path PATH [...]] [--dump-anchors]
                            [--audit-commits [N]]
                            [--baseline FILE [--update-baseline]]
                            [--changed-only [--base HEAD]]
                            [--check-install]

Exit non-zero on any error. With --no-strict, a missing regression-test
path degrades to a warning; with --allow-missing, a missing notes directory
does not fail. --seal verifies the tree, then records every archived note's
SHA-256 in `archived/manifest.json`; once sealed, any later modification or
deletion of an archived note fails verification.

--find REGEX prints the notes matching REGEX and exits without verifying; use it
instead of an index file to locate the note that owns a decision.

--for-path PATH [...] prints the notes that cite each PATH -- a note cites a
file when a backticked repo-relative path in its body equals the path or names
a directory containing it -- and exits without verifying. This is the edit-time
reverse lookup: run it on the files you are about to change so the notes
guarding them surface before the edit, not after.

--dump-anchors prints every `path::anchor` bound in any ## Verification
section, one per line, and exits. Feed it to a repo-side check that intersects
the anchors with the real test list (`cargo test -- --list`, `pytest
--collect-only`, ...) so CI can catch a renamed test that survives as an
orphan substring.

--audit-commits [N] is an advisory scan of the last N commits (default 100,
0 = all history): it warns on commits whose subject looks like a fix and that
touched real code but added neither a note under --notes-dir nor a
test-looking file. It answers "how many recent fixes went unlocked" — the gap
the format gate cannot see, since it only verifies notes that exist. The
heuristic cannot distinguish a trivial fix from a non-trivial one, so findings
are printed as a report and never fail the gate; run it periodically or in a
non-blocking CI step.

Binding checks. `## Verification` may bind a note to one exact test by writing
`path::anchor`, e.g. `tests/test_login.py::test_retry_after_lockout`; the anchor
is checked as a plain substring of that file, so the grammar stays language
neutral. A token containing `::` is only treated as a binding when the part
before the first `::` resolves to an existing file, which keeps C++ scope
resolution (`std::filesystem`, `Class::Method`) out of the grammar without a
language list. Two further checks are heuristic and therefore warnings, never
errors: a backticked bare filename that resolves nowhere under the repo, and a
backticked `snake_case`/`Pascal_Case` identifier that is absent from every test
file the note cites (likely a renamed test).

Noise controls (all generic, all optional, gate stays red on errors only):
- `--baseline FILE` suppresses known warnings so a large tree can adopt the
  gate incrementally; `--update-baseline` (re)writes FILE from the current run
  and cannot be combined with `--changed-only`. Errors are never baselined.
- `--changed-only [--base HEAD]` limits *warnings* to notes changed versus git
  <base> (untracked files included); errors are always reported for the whole
  tree. Outside a git work tree it falls back to the full tree.
- A per-note `<!-- verify-disable: name-heuristic, bare-resolution, note-ref,
  anchor -->` (or `all`, parsed from fence-stripped prose with inline code
  spans removed) disables that
  warning for that note only. `anchor` never silences the `--strict-anchors`
  error form.
- Bare-filename resolution prefers `git ls-files` (respects .gitignore, much
  faster on monorepos) and falls back to a pruned directory walk.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

LIFECYCLES = ("proposed", "implemented", "rejected", "archived")
CLASSES = ("bug-fix", "feature", "architecture", "process", "testing", "simplification")
FILENAME_RE = re.compile(r"^(\d{4})-(\d{2})-(\d{2})-.+\.md$")
SKIP_NAMES = {"AGENTS.md", "CLAUDE.md", "README.md", "manifest.json"}
MANIFEST_NAME = "manifest.json"

# Per-lifecycle status line grammar.
STATUS_RE = {
    "proposed": re.compile(r"^Status: proposed$"),
    "implemented": re.compile(r"^Status: implemented$"),
    "rejected": re.compile(r"^Status: rejected — .+$"),
    "archived": re.compile(r"^Status: implemented$"),
}

# Required ## headings beyond the universal opener `## Problem`.
REQUIRED = {
    "proposed": ["Proposal", "Acceptance criteria", "Risks"],
    "implemented": ["Decision", "Consequences"],
    "rejected": ["Proposal", "Alternatives considered"],
    "archived": ["Decision", "Consequences"],
}

BANNED_IMPLEMENTED_HEADINGS = re.compile(
    r"^## (?:Proposal\b|Plan\b|Migration plan\b|Acceptance criteria\b)", re.I
)

# Backticked path-like token that contains a directory separator.
TEST_RE = re.compile(r"`([^`\s]*[/\\][^`\s]*)`")
PROVED_RE = re.compile(r"^Proved:\s*\S", re.M)

# Tokens that are machine-absolute, never repo-relative: URI schemes,
# Windows drives, UNC/verbatim paths, POSIX roots, %-vars, and registry
# hives. A `HKLM\SOFTWARE\Foo` or `C:\App` cited in prose is context, not a
# test target, and must never hit the existence check.
MACHINE_PATH_RE = re.compile(
    r"^(?:"
    r"[A-Za-z][A-Za-z0-9+.-]*://"       # URI scheme (https://, file://)
    r"|[A-Za-z]:[\\/]"                  # Windows drive (C:\, D:/)
    r"|\\\\"                            # UNC / verbatim (\\server, \\?\)
    r"|/"                               # POSIX absolute
    r"|%[A-Za-z_][A-Za-z0-9_]*%[\\/]"   # %WINDIR%\..., %APPDATA%\...
    r"|HKEY_[A-Z_]+[\\/]"               # registry hive, long form
    r"|HK(?:LM|CU|CR|U|CC|PD)[\\/]"     # registry hive, short form
    r")", re.I)

# Optional header lines naming a successor note. Both are validated for
# resolution, self-reference, and cycles; `Partly-superseded-by` additionally
# requires a `## Superseded` section so a partially replaced decision cannot be
# read as still-current authority.
SUPERSEDE_RE = re.compile(r"^(Partly-superseded-by|Superseded-by): (\S.*)$")

# `path::anchor`, split on the first `::`.
ANCHOR_SEP = "::"

# Backticked token shaped like a bare filename: a lowercase extension, which is
# what separates `SnapshotGeometry.cs` from a dotted code identifier or a rule ID
# (`Assert.False`, `rule.Execute`, `CheckTool.Engine.Tests`, `R01.G.01` all have a
# non-lowercase or numeric suffix and are therefore never candidates).
BARE_FILE_RE = re.compile(r"`([A-Za-z0-9_][A-Za-z0-9_.+-]*\.([a-z][a-z0-9]{1,7}))`")

# Extensions that make a bare token a *file citation* rather than an API member.
# This is a list of file extensions, not of languages: `Polygon.area` (an arcpy
# attribute) and `obj.value` are excluded because `area`/`value` are not here,
# while `SnapshotGeometry.cs` is included. Extend it when a project cites a file
# type that is missing, rather than weakening the check.
SOURCE_EXTS = frozenset({
    "c", "cc", "cfg", "clj", "conf", "cpp", "cs", "css", "csv", "cxx", "dart",
    "ex", "exs", "fs", "go", "h", "hpp", "hs", "htm", "html", "ini", "java",
    "jl", "js", "json", "jsonl", "jsx", "kt", "kts", "lua", "m", "md", "ml",
    "mm", "php", "pl", "properties", "ps1", "py", "r", "rb", "rs", "sass",
    "scala", "scss", "sh", "sql", "svelte", "swift", "toml", "ts", "tsx",
    "txt", "vue", "xml", "yaml", "yml",
})

# Backticked identifier shaped like a test name in the common `snake_case` /
# `Pascal_Case` convention. Heuristic only: it feeds a warning, never an error.
NAME_RE = re.compile(r"`([A-Za-z][A-Za-z0-9]*(?:_[A-Za-z0-9]+)+)`")

# Backticked note filename, i.e. a date-prefixed `.md`. Scanning the whole note
# (not just `## Verification`) catches a note citing another note that has since
# been renamed or deleted.
NOTE_REF_RE = re.compile(r"`(\d{4}-\d{2}-\d{2}-[A-Za-z0-9._-]+\.md)`")

# Directories never walked when resolving a bare filename.
PRUNE_DIRS = frozenset({
    ".git", ".hg", ".svn", "node_modules", "__pycache__", ".venv", "venv",
    ".tox", ".mypy_cache", ".pytest_cache", "bin", "obj", "target", "dist",
    "build", ".idea", ".vs", ".next", ".cache",
})
BARE_INDEX_CAP = 200_000

# Per-note opt-out: `<!-- verify-disable: name-heuristic, bare-resolution,
# note-ref, anchor -->` or `<!-- verify-disable: all -->`. Language neutral
# (an HTML comment inside Markdown) and scoped to one note, so a noisy legacy
# note does not force a global `--no-*` flag. Parsed from fence-stripped prose
# so a fenced usage example cannot disable its own note's checks. `anchor`
# only silences the warning form: under `--strict-anchors` a missing anchor is
# always an error (errors are never suppressible, same as the baseline).
DISABLE_RE = re.compile(r"<!--\s*verify-disable:\s*([A-Za-z0-9_,\s-]+)\s*-->")
DISABLE_TOKENS = frozenset({"name-heuristic", "bare-resolution", "note-ref", "anchor", "all"})

# Heuristic blob guard: only the first N chars of each cited test file feed the
# renamed-test-name check, so a huge snapshot/fixture cannot blow up memory.
HEURISTIC_BLOB_CAP = 1_000_000

BASELINE_VERSION = 2


def valid_date(year, month, day):
    try:
        return datetime.date(int(year), int(month), int(day))
    except ValueError:
        return None


def strip_fences(text):
    out, in_fence = [], False
    for line in text.splitlines():
        if line.strip().startswith("```"):
            in_fence = not in_fence
            continue
        if not in_fence:
            out.append(line)
    return out


def read_text_safe(path):
    try:
        return path.read_text(encoding="utf-8-sig", errors="ignore")
    except OSError:
        return ""


def looks_like_test_name(token):
    """`test_...` or a token carrying an uppercase letter, so prose identifiers
    like `read_only` or `or_default` do not trip the heuristic."""
    if "_" not in token:
        return False
    return token.startswith("test_") or any(c.isupper() for c in token)


def parse_disables(text):
    """Per-note `<!-- verify-disable: ... -->` tokens, intersected with the
    known set so typos fail silently open (a typo must not disable checks).
    Inline code spans are stripped first, so a backticked usage example
    cannot disable its own note's checks (fences are stripped by the caller)."""
    text = re.sub(r"`[^`\n]*`", "", text)
    found = set()
    for m in DISABLE_RE.finditer(text):
        for tok in re.split(r"[,\s]+", m.group(1).strip().lower()):
            if tok in DISABLE_TOKENS:
                found.add(tok)
    return found


def warning_key(warning, notes, repo):
    """Portable baseline key: run-local path prefixes become placeholders, so
    the same baseline works across machines. Only anchored occurrences are
    replaced (prefix followed by a path delimiter or punctuation), so a short
    `--notes-dir notes` never rewrites the `notes` inside `test_notes.py`.
    A trivial `.` root is skipped: relative-path output is already portable."""
    key = warning.replace(os.sep, "/")
    for prefix, name in ((str(notes), "$NOTES"), (str(repo), "$REPO")):
        if not prefix or prefix == ".":
            continue
        norm = prefix.replace(os.sep, "/")
        key = re.sub(re.escape(norm) + r"(?=[/:;\"'`\s]|$)", name, key)
    return key


def load_baseline(path):
    """Returns a set of keys, an empty set when absent, or None when malformed.

    A present but mismatched `version` is malformed (fail closed); a missing
    `version` is accepted so hand-written v0 baselines keep working.
    """
    if path is None:
        return set()
    p = Path(path)
    if not p.exists():
        return set()
    try:
        data = json.loads(p.read_text(encoding="utf-8-sig"))
    except (OSError, ValueError):
        return None
    if not isinstance(data, dict) or not isinstance(data.get("warnings"), list):
        return None
    if "version" in data and data["version"] != BASELINE_VERSION:
        return None
    return set(w for w in data["warnings"] if isinstance(w, str))


def get_changed_paths(repo_root, base):
    """Absolute changed paths versus git <base> (tracked diff + untracked).

    Both listings are made cwd-relative to `repo_root` (`diff --relative`,
    `ls-files` without `--exclude-standard`) so joining them onto `repo_root`
    is sound even when `repo_root` is a subdirectory of the work tree.
    Untracked listing intentionally includes ignored files: this set only
    decides warning membership, so wider is strictly more conservative (a
    git-ignored notes dir must never silence its own warnings).
    Returns None when git is unavailable/fails, so callers fall back to the
    full tree instead of silently skipping notes.
    """
    try:
        diff = subprocess.run(
            ["git", "-C", str(repo_root), "diff", "--name-only",
             "--relative", "-z", base],
            capture_output=True, timeout=30)
        if diff.returncode != 0:
            return None
        others = subprocess.run(
            ["git", "-C", str(repo_root), "ls-files", "--others", "-z"],
            capture_output=True, timeout=30)
        if others.returncode != 0:
            return None
        changed = set()
        for raw in (diff.stdout.split(b"\x00") + others.stdout.split(b"\x00")):
            if not raw:
                continue
            rel = raw.decode("utf-8", "ignore")
            if rel:
                changed.add((Path(repo_root) / rel).resolve())
        return changed
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


class BareIndex:
    """Lazily built basename index for bare-filename resolution. Building it walks
    the repo, so it is only built once a bare candidate actually appears, and it
    reports None past the file cap instead of guessing."""

    def __init__(self, repo_root):
        self.repo_root = repo_root
        self._index = None
        self._exhausted = False

    def _from_git(self):
        """Fast path: `git ls-files` respects .gitignore and avoids walking
        ignored build trees; falls back to a walk outside a git work tree.
        Intentional difference: submodules/tarballs may resolve differently
        between the two paths; warnings-only impact, fail-open."""
        try:
            out = subprocess.run(
                ["git", "-C", str(self.repo_root), "ls-files", "-z",
                 "--cached", "--others", "--exclude-standard"],
                capture_output=True, timeout=30)
        except (OSError, ValueError, subprocess.SubprocessError):
            return None
        if out.returncode != 0:
            return None
        index, count = {}, 0
        for raw in out.stdout.split(b"\x00"):
            if not raw:
                continue
            name = raw.decode("utf-8", "ignore").rsplit("/", 1)[-1]
            if not name:
                continue
            index[name] = True
            count += 1
            if count > BARE_INDEX_CAP:
                self._exhausted = True
                return None
        return index

    def get(self):
        if self._index is None and not self._exhausted:
            git_index = self._from_git()
            if self._exhausted:
                # Over the cap on the fast path: report None without paying
                # for a second full walk that would exceed it too.
                return None
            if git_index is not None:
                self._index = git_index
                return self._index
            index, count = {}, 0
            for root, dirs, files in os.walk(self.repo_root):
                dirs[:] = [d for d in dirs if d not in PRUNE_DIRS]
                for name in files:
                    index[name] = True
                    count += 1
                    if count > BARE_INDEX_CAP:
                        self._exhausted = True
                        return None
            self._index = index
        return self._index


def parse_header(lines, lifecycle, path):
    """Validate the header block; returns (errors, supersedes).

    Header grammar: title, blank, `Status:`, optional `Archived:` (archived only),
    zero or more supersede lines, blank.
    """
    errors = []
    supersedes = []

    if not re.match(r"^# Agent Note: \S", lines[0]):
        errors.append(f"{path}: L1 must be `# Agent Note: <title>`")
    if lines[1].strip() != "":
        errors.append(f"{path}: L2 must be blank")

    want = STATUS_RE[lifecycle]
    if not want.match(lines[2].strip()):
        errors.append(
            f"{path}: L3 must match the `{lifecycle}/` status grammar ({want.pattern})"
        )

    i = 3
    if lifecycle == "archived":
        if len(lines) <= i:
            errors.append(f"{path}: archived notes need `Archived: YYYY-MM-DD` after `Status:`")
            return errors, supersedes
        archived_match = re.match(r"^Archived: (\d{4})-(\d{2})-(\d{2})$", lines[i].strip())
        if not archived_match:
            errors.append(f"{path}: L4 must be `Archived: YYYY-MM-DD`")
        else:
            archive_date = valid_date(*archived_match.groups())
            today = datetime.date.today()
            if not archive_date:
                errors.append(f"{path}: archived date is not a valid calendar date")
            elif archive_date > today:
                errors.append(f"{path}: archived date cannot be in the future")
        i += 1

    while i < len(lines):
        match = SUPERSEDE_RE.match(lines[i].strip())
        if not match:
            break
        supersedes.append((match.group(1), match.group(2).strip()))
        i += 1

    if i >= len(lines):
        errors.append(f"{path}: the header block must end with a blank line")
    elif lines[i].strip() != "":
        errors.append(
            f"{path}: L{i + 1} must be blank (the header block ends with a blank line)"
        )

    return errors, supersedes


def note_digest(path):
    """归档 note 的内容摘要，**行尾归一化后**再哈希。

    仓库以 LF 存 blob；Windows 检出（core.autocrlf）会把工作区写成 CRLF。
    按原始字节密封会让同一份内容在不同平台得出不同摘要——密封清单在
    Windows 上生成、在 Linux CI 复核时报「archived note was modified after
    sealing」（实际只是检出换行差异）。密封的语义是内容未变，行尾不算。
    """
    data = path.read_bytes().replace(b"\r\n", b"\n")
    return hashlib.sha256(data).hexdigest()


def load_manifest(notes):
    """The archived-freeze manifest, or None when the file is not a JSON object."""
    path = notes / "archived" / MANIFEST_NAME
    if not path.exists():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def check_verification(path, ver, repo_root, strict, opts, bare_index,
                       disables=frozenset(), file_cache=None):
    """Validate a `## Verification` section. Returns (errors, warnings)."""
    errors, warnings = [], []
    if file_cache is None:
        file_cache = {}

    def cached_text(rel):
        if rel not in file_cache:
            file_cache[rel] = read_text_safe(repo_root / rel)
        return file_cache[rel]

    if not PROVED_RE.search(ver):
        errors.append(
            f"{path}: `## Verification` must contain a `Proved:` line recording the red-run proof"
        )
    if "<what you re-broke>" in ver:
        errors.append(f"{path}: `Proved:` line still carries the template placeholder")

    # `Proved:` lines cite evidence as well as tests: a backticked path on a
    # `Proved:` line that does not exist (e.g. `target/foo.log` that `cargo
    # clean` deletes) is evidence, so it must not fail a fresh clone — it gets
    # a warning instead. A `Proved:` token that does exist still counts as a
    # binding target like any other.
    proved_tokens = set()
    raw = []
    for line in ver.splitlines():
        found = [t.strip() for t in TEST_RE.findall(line)]
        raw.extend(found)
        if PROVED_RE.match(line):
            proved_tokens.update(found)

    targets, anchors = [], []
    for token in raw:
        if MACHINE_PATH_RE.match(token):
            continue
        left, sep, right = token.partition(ANCHOR_SEP)
        if sep and right and (repo_root / left).is_file():
            targets.append(left)
            anchors.append((left, right))
        else:
            targets.append(token)

    if not targets:
        errors.append(
            f"{path}: `## Verification` must reference at least one test path in backticks"
        )
        return errors, warnings

    evidence_missing = sorted({
        t.partition(ANCHOR_SEP)[0].strip()
        for t in proved_tokens
        if ":" not in t.partition(ANCHOR_SEP)[0]
        and not MACHINE_PATH_RE.match(t)
        and not (repo_root / t.partition(ANCHOR_SEP)[0]).exists()
    })
    if evidence_missing:
        warnings.append(
            f"WARNING: {path}: `Proved:` cites evidence not present in the "
            f"repo: {', '.join(evidence_missing)}; commit the red-run output "
            f"somewhere durable (e.g. a `<notes>-evidence/` sibling of the "
            f"notes tree) or record the natural-red source inline")

    missing = [t for t in targets
               if not (repo_root / t).exists() and t not in proved_tokens]
    if missing:
        msg = f"{path}: verification target(s) not found: {', '.join(missing)}"
        if strict:
            errors.append("ERROR: " + msg)
        else:
            warnings.append("WARNING: " + msg)

    if anchors:
        anchor_disabled = "anchor" in disables or "all" in disables
        for left, right in anchors:
            if right not in cached_text(left):
                msg = f"{path}: anchor `{right}` not found in `{left}`"
                if opts.strict_anchors:
                    # Errors are never suppressible: a per-note `anchor`
                    # disable only silences the warning form. Clean the
                    # binding before promoting a tree to --strict-anchors.
                    errors.append("ERROR: " + msg)
                elif not anchor_disabled:
                    warnings.append("WARNING: " + msg)

    if (not opts.no_name_heuristic and "name-heuristic" not in disables
            and "all" not in disables):
        cited = [(t, repo_root / t) for t in targets if (repo_root / t).is_file()]
        if cited:
            blob = "\n".join(cached_text(t)[:HEURISTIC_BLOB_CAP] for t, _ in cited)
            for token in sorted(set(NAME_RE.findall(ver))):
                if looks_like_test_name(token) and token not in blob:
                    warnings.append(
                        f"WARNING: {path}: `{token}` is cited in `## Verification` but absent "
                        f"from {', '.join(t for t, _ in cited)}; if it is a test name, cite "
                        f"`{cited[0][0]}::{token}` so the binding is checkable"
                    )

    if (not opts.no_bare_resolution and "bare-resolution" not in disables
            and "all" not in disables):
        # A dangling note reference (`2026-09-01-*.md`) is reported once by the
        # note-ref check; exclude it here so one token never yields two warnings.
        note_refs = set(NOTE_REF_RE.findall(ver))
        bare = sorted({tok for tok, ext in BARE_FILE_RE.findall(ver)
                       if ext in SOURCE_EXTS and tok not in note_refs})
        if bare:
            index = bare_index.get()
            if index is None:
                warnings.append(
                    f"WARNING: {path}: bare-filename resolution skipped (repo exceeds "
                    f"{BARE_INDEX_CAP} files)"
                )
            else:
                for token in bare:
                    if token not in index:
                        warnings.append(
                            f"WARNING: {path}: `{token}` is a bare filename that resolves "
                            f"nowhere under {repo_root}; cite a path"
                        )

    return errors, warnings


def check_file(path, lifecycle, cls, repo_root, strict, opts, bare_index,
             note_names=None, disables=None, file_cache=None):
    """Returns (errors, warnings, supersedes)."""
    errors, warnings = [], []
    try:
        text = path.read_text(encoding="utf-8-sig")
    except (OSError, UnicodeDecodeError) as e:
        return [f"{path}: cannot read: {e}"], [], []

    if disables is None:
        disables = parse_disables("\n".join(strip_fences(text)))

    lines = text.splitlines()
    if len(lines) < 4:
        return [f"{path}: header block too short"], [], []
    header_errors, supersedes = parse_header(lines, lifecycle, path)
    errors.extend(header_errors)

    # Exactly one Status: line in the entire prose, and it matches the header.
    prose = strip_fences(text)
    status_lines = [ln for ln in prose if ln.startswith("Status:")]
    if len(status_lines) != 1:
        errors.append(f"{path}: exactly one `Status:` line required")
    elif lines and status_lines[0] != lines[2]:
        errors.append(f"{path}: the only `Status:` line must be L3")

    h2s = [ln.strip()[3:].strip() for ln in prose if ln.startswith("## ")]
    if not h2s or h2s[0] != "Problem":
        errors.append(f"{path}: body must open with `## Problem`")

    for sec in REQUIRED[lifecycle]:
        if sec not in h2s:
            errors.append(f"{path}: missing `## {sec}` for `{lifecycle}/`")

    if "Alternatives considered" not in h2s:
        errors.append(f"{path}: missing mandatory `## Alternatives considered`")

    if lifecycle in ("implemented", "archived"):
        for h2 in h2s:
            if BANNED_IMPLEMENTED_HEADINGS.match(f"## {h2}"):
                errors.append(
                    f"{path}: `{h2}` is a proposal-era heading; implemented/archived notes state what is"
                )

    if any(kind == "Partly-superseded-by" for kind, _ in supersedes) and "Superseded" not in h2s:
        errors.append(
            f"{path}: `Partly-superseded-by` requires a `## Superseded` section stating "
            f"what still holds and what no longer does"
        )

    if cls == "bug-fix" and lifecycle in ("implemented", "archived"):
        if "Verification" not in h2s:
            errors.append(f"{path}: bug-fix notes require `## Verification`")
        else:
            body = "\n".join(prose)
            ver = body.split("## Verification", 1)[1]
            ver = re.split(r"^## ", ver, maxsplit=1, flags=re.M)[0]
            e, w = check_verification(path, ver, repo_root, strict, opts, bare_index,
                                      disables, file_cache)
            errors.extend(e)
            warnings.extend(w)

    if note_names is not None and "note-ref" not in disables and "all" not in disables:
        for ref in sorted(set(NOTE_REF_RE.findall(text))):
            if ref != path.name and ref not in note_names:
                warnings.append(
                    f"WARNING: {path}: references note `{ref}`, which is not in this tree "
                    f"(renamed or deleted?)"
                )

    # Filename date must be a real calendar date and not in the future.
    filename_match = FILENAME_RE.match(path.name)
    if filename_match:
        file_date = valid_date(*filename_match.groups())
        today = datetime.date.today()
        if not file_date:
            errors.append(f"{path}: filename date is not a valid calendar date")
        elif file_date > today:
            errors.append(f"{path}: filename date cannot be in the future")
        elif lifecycle == "archived":
            archive_match = re.match(r"^Archived: (\d{4})-(\d{2})-(\d{2})$", lines[3].strip() or "")
            if archive_match:
                archive_date = valid_date(*archive_match.groups())
                if archive_date and file_date and archive_date < file_date:
                    errors.append(
                        f"{path}: archived date `{lines[3].strip()}` is before filename date `{path.name[:10]}`"
                    )

    return errors, warnings, supersedes


def check_supersede_graph(notes, edges):
    """Validate every supersede pointer: resolution, self-reference, cycles, and
    the archived-folder expectation for a fully superseded note."""
    errors = []
    by_name = {}
    for md, lifecycle, _cls in notes:
        by_name.setdefault(md.name, (md, lifecycle))

    graph = {}
    for src_name, kind, target, src_lifecycle in edges:
        if target == src_name:
            errors.append(f"{src_name}: `{kind}: {target}` points at itself")
            continue
        if target not in by_name:
            errors.append(
                f"{src_name}: `{kind}: {target}` does not resolve to a note in this tree"
            )
            continue
        if kind == "Superseded-by" and src_lifecycle != "archived":
            errors.append(
                f"{src_name}: `Superseded-by` is set but the note is not under `archived/`; "
                f"a fully superseded note is consolidated into its successor and archived"
            )
        graph.setdefault(src_name, []).append(target)

    # Cycle detection over the supersede graph.
    WHITE, GREY, BLACK = 0, 1, 2
    colour = {}

    def visit(node, stack):
        colour[node] = GREY
        stack.append(node)
        for nxt in graph.get(node, []):
            if colour.get(nxt, WHITE) == GREY:
                loop = stack[stack.index(nxt):] + [nxt]
                errors.append("supersede cycle: " + " -> ".join(loop))
            elif colour.get(nxt, WHITE) == WHITE:
                visit(nxt, stack)
        stack.pop()
        colour[node] = BLACK

    for node in sorted(graph):
        if colour.get(node, WHITE) == WHITE:
            visit(node, [])

    return errors


def find_notes(notes, pattern):
    """Print notes matching `pattern` (title, body, or filename)."""
    rx = re.compile(pattern, re.I)
    hits = 0
    for top in sorted(p for p in notes.iterdir() if p.is_dir() and p.name in LIFECYCLES):
        for second in sorted(p for p in top.iterdir() if p.is_dir() and p.name in CLASSES):
            for md in sorted(second.glob("*.md")):
                text = read_text_safe(md)
                if not (rx.search(md.name) or rx.search(text)):
                    continue
                hits += 1
                lines = text.splitlines()
                title = lines[0][len("# Agent Note: "):] if lines and lines[0].startswith("# Agent Note: ") else md.name
                decision = ""
                if "## Decision" in lines:
                    rest = lines[lines.index("## Decision") + 1:]
                    decision = next((l.strip() for l in rest if l.strip()), "")
                print(md.relative_to(notes).as_posix())
                print(f"    title:    {title}")
                if decision:
                    print(f"    decision: {decision[:160]}")
                for target in TEST_RE.findall(text):
                    print(f"    test:     {target.strip()}")
                print()
    print(f"{hits} note(s) matched `{pattern}`")
    return 0


def _norm_repo_path(token):
    """Normalize a backticked repo-relative path token to lowercase posix
    parts; None when the token is not a usable path (URL, absolute,
    registry hive, `..`, or a `path::anchor` remainder)."""
    token = token.partition(ANCHOR_SEP)[0].strip()
    if not token or ":" in token or token.startswith(("/", "\\", "~")):
        return None
    if MACHINE_PATH_RE.match(token):
        return None
    parts = [p for p in re.split(r"[/\\]+", token) if p not in ("", ".")]
    if not parts or any(p == ".." for p in parts):
        return None
    return tuple(p.lower() for p in parts)


def covered_notes(notes, paths):
    """Print the notes citing each PATH: a note cites a file when a backticked
    repo-relative path anywhere in its body equals PATH or names a directory
    containing it. Archived and rejected notes are labelled, since they are
    context, not current locks."""
    wanted = []
    for raw in paths:
        norm = _norm_repo_path(raw)
        if norm is None:
            print(f"{raw}: not a repo-relative path, skipped")
        else:
            wanted.append((raw, norm))
    hits = 0
    for top in sorted(p for p in notes.iterdir() if p.is_dir() and p.name in LIFECYCLES):
        for second in sorted(p for p in top.iterdir() if p.is_dir() and p.name in CLASSES):
            for md in sorted(second.glob("*.md")):
                if md.name.endswith(".zh.md") or md.name in SKIP_NAMES:
                    continue
                text = read_text_safe(md)
                cited = set()
                for token in TEST_RE.findall(text):
                    norm = _norm_repo_path(token)
                    if norm:
                        cited.add(norm)
                matched = []
                for raw, norm in wanted:
                    for c in cited:
                        if norm == c or norm[: len(c)] == c:
                            matched.append(raw)
                            break
                if not matched:
                    continue
                hits += 1
                lines = text.splitlines()
                title = lines[0][len("# Agent Note: "):] if lines and lines[0].startswith("# Agent Note: ") else md.name
                state = {"implemented": "locks", "archived": "history",
                         "proposed": "proposed", "rejected": "rejected"}.get(top.name, top.name)
                print(md.relative_to(notes).as_posix())
                print(f"    title:  {title}")
                print(f"    state:  {state}")
                print(f"    covers: {', '.join(matched)}")
                print()
    print(f"{hits} note(s) cite {len(wanted)} queried path(s)")
    return 0


def dump_anchors(notes):
    """Print every `path::anchor` bound in a ## Verification section."""
    count = 0
    for top in sorted(p for p in notes.iterdir() if p.is_dir() and p.name in LIFECYCLES):
        for second in sorted(p for p in top.iterdir() if p.is_dir() and p.name in CLASSES):
            for md in sorted(second.glob("*.md")):
                text = "\n".join(strip_fences(read_text_safe(md)))
                if "## Verification" not in text:
                    continue
                ver = text.split("## Verification", 1)[1]
                for token in TEST_RE.findall(ver):
                    left, sep, right = token.strip().partition(ANCHOR_SEP)
                    if sep and right:
                        count += 1
                        print(f"{left.strip()}::{right.strip()}")
    print(f"{count} anchor(s)", file=sys.stderr)
    return 0


FIXISH_RE = re.compile(
    r"\b(?:fix(?:e[sd])?|bug(?:fix)?|regress\w*|revert|hotfix|patch)\b", re.I)
TESTY_RE = re.compile(
    r"(^|/)(tests?|__tests__|spec)(/|$)|(_test|\.test|\.spec)\.[a-z0-9]+$|"
    r"(^|/)test_[^/]*\.[a-z0-9]+$", re.I)
DOCPATH_RE = re.compile(r"(^|/)docs?/|\.(md|markdown|rst|txt|adoc)$", re.I)


def _audit_paths(repo, sha):
    """Files touched by a commit (paths repo-relative, posix)."""
    try:
        r = subprocess.run(
            ["git", "-C", str(repo), "diff-tree", "--no-commit-id",
             "--name-only", "-r", "--root", sha],
            capture_output=True, encoding="utf-8", errors="replace",
            timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return []
    if r.returncode != 0:
        return []
    return [l.strip() for l in r.stdout.splitlines() if l.strip()]


def audit_commits(notes, repo, limit):
    """Heuristic 'should have had a note' scan over recent history.

    Flags commits whose subject looks like a fix AND which touched real code
    but added neither a note nor a test-looking file. Advisory only — the
    heuristic cannot tell a trivial fix from a non-trivial one, so output is a
    report, never a gate failure. Returns 0 always.
    """
    try:
        notes_rel = notes.resolve().relative_to(repo.resolve()).as_posix()
    except (OSError, ValueError):
        print(f"notes dir {notes} is not inside repo root {repo}; "
              f"cannot audit commits against it")
        return 0
    notes_prefix = notes_rel.rstrip("/") + "/"
    try:
        inside = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "--is-inside-work-tree"],
            capture_output=True, encoding="utf-8", errors="replace",
            timeout=15)
    except (OSError, subprocess.TimeoutExpired):
        inside = None
    if inside is None or inside.returncode != 0:
        print(f"{repo} is not a git work tree; --audit-commits needs history")
        return 0

    log_args = ["git", "-C", str(repo), "log", "--no-merges",
                "--format=%H%x09%s"]
    if limit and limit > 0:
        log_args += ["-n", str(limit)]
    try:
        r = subprocess.run(log_args, capture_output=True, encoding="utf-8",
                           errors="replace", timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        print("git log failed", file=sys.stderr)
        return 1
    if r.returncode != 0:
        print(f"git log failed: {r.stderr.strip()}", file=sys.stderr)
        return 1

    flagged, fixish, skipped = [], 0, 0
    for line in r.stdout.splitlines():
        if "\x09" not in line:
            continue
        sha, subject = line.split("\x09", 1)
        if not FIXISH_RE.search(subject):
            continue
        fixish += 1
        paths = _audit_paths(repo, sha)
        touched_note = any(p.startswith(notes_prefix) and p.endswith(".md")
                           for p in paths)
        if touched_note:
            continue
        code = [p for p in paths
                if not TESTY_RE.search(p) and not DOCPATH_RE.search(p)
                and not p.startswith(notes_prefix)]
        tests = [p for p in paths if TESTY_RE.search(p)]
        if not code:
            # docs-only or test-only "fix" commits carry no behavior change.
            skipped += 1
            continue
        short = sha[:10]
        if tests:
            flagged.append(
                f"{short} {subject} -- touched code + test file(s) "
                f"({tests[0]}) but no note")
        else:
            flagged.append(
                f"{short} {subject} -- touched code but no note and no "
                f"test file ({code[0]}{'...' if len(code) > 1 else ''})")

    for f in flagged:
        print(f"WARNING: {f}")
    print(f"{fixish} fix-looking commit(s) scanned "
          f"({'all history' if not limit or limit <= 0 else f'last {limit}'}), "
          f"{fixish - len(flagged) - skipped} carried a note, "
          f"{skipped} touched no code, {len(flagged)} look unnoted")
    return 0


def check_install(notes, repo):
    """Read-only wiring self-check: the tree exists and some runner invokes it.

    A packaging/release-script wiring cannot be detected generically, so the
    gate counts a pre-commit hook or a CI config that mentions verify-notes.
    Prints one line per check plus a verdict; returns 0 only when installed.
    """
    wired = True

    if notes.is_dir():
        print(f"notes dir: OK ({notes})")
    else:
        print(f"notes dir: MISSING ({notes}) -- create it with a one-line "
              f"README.md so the gate has a tree on a fresh clone")
        wired = False

    hook_ok = False
    # Check the default location plus `.githooks/`, the conventional tracked
    # hooks dir for repos that wire `core.hooksPath`. A hook counts as a gate
    # only when some line invokes the verifier without a query-mode flag —
    # a hook that merely prints `--for-path` reminders is a reminder, not a
    # gate, and must not satisfy the install check.
    QUERY_FLAGS = ("--for-path", "--find", "--dump-anchors",
                   "--check-install", "--audit-commits")
    for hook in (repo / ".git" / "hooks" / "pre-commit",
                 repo / ".githooks" / "pre-commit"):
        try:
            lines = hook.read_text(
                encoding="utf-8-sig", errors="ignore").splitlines()
        except OSError:
            continue
        for line in lines:
            if ("verify-notes" in line and "--notes-dir" in line
                    and not any(f in line for f in QUERY_FLAGS)):
                hook_ok = True
    print(f"pre-commit hook: {'OK' if hook_ok else 'MISSING'} "
          f"(.git/hooks or .githooks; query-only invocations do not count)")

    ci_hits = []
    workflows = repo / ".github" / "workflows"
    if workflows.is_dir():
        for wf in sorted(workflows.glob("*.yml")) + sorted(workflows.glob("*.yaml")):
            try:
                if "verify-notes" in wf.read_text(encoding="utf-8-sig", errors="ignore"):
                    ci_hits.append(wf.relative_to(repo).as_posix())
            except OSError:
                continue
    for candidate in (".gitlab-ci.yml", "azure-pipelines.yml"):
        p = repo / candidate
        try:
            if p.is_file() and "verify-notes" in p.read_text(
                    encoding="utf-8-sig", errors="ignore"):
                ci_hits.append(candidate)
        except OSError:
            continue
    print(f"CI config: {', '.join(ci_hits) if ci_hits else 'NOT FOUND'} "
          f"(a packaging-script wiring cannot be auto-detected)")

    # Agent-facing docs: the gate can be perfectly wired and still useless when
    # no standing doc tells agents the tree exists. Advisory only — a missing
    # pointer is a discoverability gap, not a broken gate.
    try:
        doc_rel = notes.resolve().relative_to(repo.resolve()).as_posix()
    except (OSError, ValueError):
        doc_rel = notes.as_posix()
    doc_hit = None
    for candidate in ("AGENTS.md", "CLAUDE.md", "README.md",
                      "docs/GOAL.md", "docs/HANDOFF.md"):
        p = repo / candidate
        try:
            text = p.read_text(encoding="utf-8-sig", errors="ignore")
        except OSError:
            continue
        if doc_rel in text or "verify-notes" in text or ".agents/notes" in text:
            doc_hit = candidate
            break
    if doc_hit:
        print(f"agent doc pointer: OK ({doc_hit} mentions {doc_rel})")
    else:
        print(f"agent doc pointer: MISSING — no AGENTS.md/CLAUDE.md/README.md "
              f"(or docs/GOAL.md, docs/HANDOFF.md) mentions `{doc_rel}`; agents "
              f"will never know the notes exist. Add the standing rule plus the "
              f"`--for-path` reverse lookup (see SKILL.md 'Installing into a "
              f"repository'); pointers only, never copied decision content")

    if wired and (hook_ok or ci_hits):
        print(f"INSTALLED: {notes} is verified by "
              f"{'the pre-commit hook' if hook_ok else 'CI'}")
        return 0
    print("NOT INSTALLED: wire the gate with a pre-commit hook or a CI job "
          "(see SKILL.md 'Installing into a repository'), then re-run "
          "--check-install")
    return 1


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--notes-dir", default=os.environ.get("NOTES_DIR", ".agents/notes"))
    ap.add_argument("--repo-root", default=".")
    ap.add_argument("--no-strict", action="store_true")
    ap.add_argument(
        "--allow-missing",
        action="store_true",
        help="treat a missing notes directory as OK (not recommended for CI)",
    )
    ap.add_argument(
        "--seal",
        action="store_true",
        help="record the SHA-256 of every archived note in archived/manifest.json",
    )
    ap.add_argument(
        "--strict-anchors",
        action="store_true",
        help="make a `path::anchor` whose anchor is missing from that file an error",
    )
    ap.add_argument(
        "--no-name-heuristic",
        action="store_true",
        help="skip the renamed-test-name warning (it is a language-shaped heuristic)",
    )
    ap.add_argument(
        "--no-bare-resolution",
        action="store_true",
        help="skip resolving backticked bare filenames (avoids the repo walk)",
    )
    ap.add_argument(
        "--find",
        metavar="REGEX",
        help="print notes matching REGEX (title, body, or filename) and exit",
    )
    ap.add_argument(
        "--for-path",
        metavar="PATH",
        nargs="+",
        help="print the notes citing PATH (a file under the cited path, or "
             "the cited path itself) and exit without verifying",
    )
    ap.add_argument(
        "--dump-anchors",
        action="store_true",
        help="print every `path::anchor` bound in ## Verification, one per "
             "line, for a repo-side check against the real test list, and exit",
    )
    ap.add_argument(
        "--audit-commits",
        metavar="N",
        type=int,
        nargs="?",
        const=100,
        default=None,
        help="advisory scan of the last N commits (default 100, 0 = all): "
             "warn on fix-looking commits that touched code but added "
             "neither a note nor a test file, then exit",
    )
    ap.add_argument(
        "--baseline",
        metavar="FILE",
        default=None,
        help="suppress warnings listed in FILE (JSON {warnings: [...]}, "
             "portable $NOTES/$REPO keys); errors are never baselined",
    )
    ap.add_argument(
        "--update-baseline",
        action="store_true",
        help="write current warnings to --baseline and pass; use once to "
             "adopt the gate incrementally, then keep the file in version control",
    )
    ap.add_argument(
        "--changed-only",
        action="store_true",
        help="limit *warnings* to notes changed versus git <base> "
             "(untracked included); errors always cover the whole tree",
    )
    ap.add_argument(
        "--base",
        default=None,
        help="git ref for --changed-only (default: HEAD; ignored without "
             "--changed-only)",
    )
    ap.add_argument(
        "--check-install",
        action="store_true",
        help="read-only wiring self-check: report whether the notes tree and "
             "a runner (pre-commit hook or CI config) exist, then exit",
    )
    args = ap.parse_args(argv)

    if args.update_baseline and not args.baseline:
        ap.error("--update-baseline requires --baseline FILE")
    if args.update_baseline and args.changed_only:
        ap.error("--update-baseline cannot be combined with --changed-only: "
                 "a baseline written from a partial tree would be incomplete")
    if args.base is not None and not args.changed_only:
        print("ignoring --base without --changed-only", file=sys.stderr)
    base = args.base or "HEAD"

    notes = Path(args.notes_dir)
    repo = Path(args.repo_root)
    strict = not args.no_strict
    errors, warnings = [], []
    archived = []

    if args.check_install:
        return check_install(notes, repo)

    if not notes.is_dir():
        msg = f"no notes dir at {notes}"
        if args.allow_missing:
            print(f"{msg}, nothing to verify (allow-missing)")
            return 0
        print(f"{msg}", file=sys.stderr)
        return 1

    if args.find:
        return find_notes(notes, args.find)

    if args.for_path:
        return covered_notes(notes, args.for_path)

    if args.dump_anchors:
        return dump_anchors(notes)

    if args.audit_commits is not None:
        return audit_commits(notes, repo, args.audit_commits)

    if (notes / "INDEX.md").exists():
        errors.append(f"{notes}/INDEX.md: no centralized index, the tree is the index")

    collected, edges = [], []
    bare_index = BareIndex(repo)
    file_cache = {}
    note_names = {p.name for p in notes.rglob("*.md")}

    changed = None
    if args.changed_only:
        changed = get_changed_paths(repo, base)
        if changed is None:
            print("changed-only: git unavailable, checking the full tree",
                  file=sys.stderr)

    def warnings_for_note(md):
        """False only when --changed-only can prove this note is untouched.

        Notes outside the repo tree stay enabled (conservative: never silence
        what git cannot see).
        """
        if changed is None:
            return True
        try:
            resolved = md.resolve()
        except OSError:
            return True
        try:
            resolved.relative_to(repo.resolve())
        except (OSError, ValueError):
            return True
        return resolved in changed
    for top in sorted(p for p in notes.iterdir() if p.name not in SKIP_NAMES):
        if not top.is_dir():
            errors.append(f"{notes}/{top.name}: unexpected file at notes root")
            continue
        if top.name not in LIFECYCLES:
            errors.append(f"{notes}/{top.name}: unknown lifecycle folder")
            continue
        for second in sorted(top.iterdir()):
            if second.name in SKIP_NAMES:
                continue
            if second.is_file():
                errors.append(f"{second}: notes must live at `{top.name}/<class>/yyyy-mm-dd-topic.md`")
                continue
            if second.name not in CLASSES:
                errors.append(f"{second}: unknown class folder (allowed: {', '.join(CLASSES)})")
                continue
            for stray in sorted(second.rglob("*")):
                if stray.is_file() and stray.suffix != ".md" and stray.name not in SKIP_NAMES:
                    errors.append(f"{stray}: only `.md` notes live under `{top.name}/{second.name}/`")
            for md in sorted(second.rglob("*.md")):
                if md.name.endswith(".zh.md"):
                    continue
                rel = md.relative_to(second)
                if len(rel.parts) > 1:
                    errors.append(f"{md}: notes must live directly in `{top.name}/{second.name}/`, no subdirectories")
                    continue
                if not FILENAME_RE.match(md.name):
                    errors.append(f"{md}: filename must be `yyyy-mm-dd-topic.md`")
                    continue
                if top.name == "archived":
                    archived.append((md, md.relative_to(notes).as_posix()))
                e, w, supersedes = check_file(md, top.name, second.name, repo, strict, args,
                                              bare_index, note_names,
                                              file_cache=file_cache)
                errors.extend(e)
                if warnings_for_note(md):
                    warnings.extend(w)
                collected.append((md, top.name, second.name))
                for kind, target in supersedes:
                    edges.append((md.name, kind, target, top.name))

    by_basename = {}
    for md, _lifecycle, _cls in collected:
        by_basename.setdefault(md.name, []).append(md)
    for name in sorted(by_basename):
        if len(by_basename[name]) > 1:
            dupes = ", ".join(str(p) for p in sorted(by_basename[name]))
            errors.append(
                f"{name}: duplicate note filename in {dupes}; basenames are "
                f"the supersede/reference key and must be unique across the tree"
            )

    errors.extend(check_supersede_graph(collected, edges))

    manifest = load_manifest(notes)
    if manifest is None:
        errors.append(f"{notes}/archived/{MANIFEST_NAME}: manifest must be a JSON object")
    else:
        if args.seal and not errors:
            # 同时覆盖「新增」与「摘要变化」两种：后者是显式的重封（例如行尾
            # 归一化迁移、工具摘要算法升级）。逐条打印被重封的路径，改动在
            # 评审里可见；其余校验错误仍然阻止密封（上面的 `not errors`）。
            for md, rel in archived:
                digest = note_digest(md)
                if manifest.get(rel) == digest:
                    continue
                manifest[rel] = digest
                print(f"sealed {rel}")
            (notes / "archived" / MANIFEST_NAME).write_text(
                json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        live = {rel for _, rel in archived}
        for rel in sorted(set(manifest) - live):
            errors.append(f"{notes}/{rel}: sealed archived note is missing")
        for md, rel in archived:
            if rel not in manifest:
                errors.append(f"{notes}/{rel}: archived note is not sealed (run with --seal)")
            elif manifest[rel] != note_digest(md):
                errors.append(f"{notes}/{rel}: archived note was modified after sealing")

    if args.baseline or args.update_baseline:
        baseline = load_baseline(args.baseline)
        if baseline is None:
            errors.append(f"{args.baseline}: baseline must be a JSON object "
                          f"{{version: {BASELINE_VERSION}, warnings: [...]}}")
        elif args.update_baseline:
            keys = sorted(warning_key(w, notes, repo) for w in warnings)
            Path(args.baseline).write_text(
                json.dumps({"version": BASELINE_VERSION, "warnings": keys},
                           indent=2, sort_keys=True) + "\n", encoding="utf-8")
            print(f"wrote {len(keys)} warning(s) to baseline {args.baseline}")
            warnings = []
        else:
            kept = [w for w in warnings
                    if warning_key(w, notes, repo) not in baseline]
            suppressed = len(warnings) - len(kept)
            if suppressed:
                print(f"suppressed {suppressed} known warning(s) via baseline "
                      f"{args.baseline}")
            warnings = kept

    for w in warnings:
        print(w)
    if errors:
        print(f"\n{len(errors)} error(s):")
        for e in errors:
            print(f"  {e}")
        return 1
    print(f"OK: regression notes under {notes} verified")
    return 0


if __name__ == "__main__":
    sys.exit(main())
