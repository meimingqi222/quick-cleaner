# Agent Note: Incremental index persistence encodes only overrides

Status: implemented

## Problem

A loaded macOS index keeps its v7 base mapped, and every write lands in
`overrides`. `PersistIncremental` still loaded the index again, read and
decoded the whole base a second time, validated every node twice, and counted
every node. On a 2M-node base with 20 changed files, it took 2.85s and
allocated 1.29 GB, with a 727 MB heap peak. Rust builds the delta from its
overrides alone. Load also touched every mapped page and kept them resident.

## Decision

`persistOverrides` in `internal/core/disk_index_cache_overrides.go` runs
before the whole-tree validation in `persist`. It requires all of these:

- the tree's mapped base has the same checksum as the base on disk;
- the base's required coverage sidecar still restores;
- any existing delta binds to that base and is not newer than this event.

`validateOverrides` checks only the overridden and appended nodes. Base
identity (parent and name) must be unchanged. Appended parents must point
backwards. A base directory may not lose its directory status while it keeps
live children. File and directory counts start from the checksum-verified
base header and are adjusted per change. `writeDiskIndexDelta` is the writer
that both paths share. When any precondition is not known to hold, the full
path runs and makes the decision. After decode, load releases the mapped
pages (`MADV_DONTNEED` + `MADV_FREE`), and the spill output is released
before munmap, both as Rust does.

## Alternatives considered

- Make `validateDiskTree` and the counts cheaper. Rejected: they still walk
  every node, and each mapped `slot()` decodes a name.
- Trust the header counts without checking override flags. Rejected:
  tombstones and type changes would leave the counts wrong.

## Consequences

On the same 2M-node base: 28ms and 0.02 MB. Delta bytes are identical to the
full encoder. Missing coverage, a replaced base, a stale or unreadable delta,
identity changes, forward parents, and large deltas still take the full path.

## Verification

- `internal/core/disk_index_cache_overrides_test.go::TestIncrementalPersistDoesNotCopyTheBase`
- `internal/core/disk_index_cache_overrides_test.go::TestOverrideDeltaMatchesFullEncoderByteForByte`

Proved: with the fast path disabled, `TestIncrementalPersistDoesNotCopyTheBase`
failed at `incremental persist allocated 142370656 bytes for a 6589160-byte
base`; see `docs/agent-notes-evidence/2026-10-24-override-persist-red.log`.
With the fast path restored, both tests pass; see
`docs/agent-notes-evidence/2026-10-24-override-persist-green.log`. These run
as a linux test binary under WSL, because the mapped index is darwin/linux only.
