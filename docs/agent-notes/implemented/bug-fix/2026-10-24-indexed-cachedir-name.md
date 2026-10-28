# Agent Note: Indexed discovery opens CACHEDIR.TAG only when the tree names it

Status: implemented

## Problem

An elevated Go junk scan took 2m9s in discovery after the MFT parses themselves
finished in a few seconds. Rust on the same volumes finished discovery in 4.6s.
The indexed walk called CreateFile for `CACHEDIR.TAG` in every directory.

## Decision

In `internal/core/devscan.go`, `treeNamesCacheDirTag` checks the in-memory
children of a directory before `hasCacheDirectoryTag` opens the file. A tree that does not list that filename
is not a tag. The walk channel still has no tree, so it still opens the file.
Signature checks and the installation-candidate rejection stay as they are.
Product volume scans pass topN 0, so they skip the CLI directory ranking.

## Alternatives considered

Opening the tag file for every directory matches the walk channel and would
keep finding a tag the index had not recorded, at the cost of one failed open
per directory. Skipping signature reads entirely would accept a forged name.
Doing the name check only on Windows would leave the macOS index on the same
path. All rejected.

## Consequences

Indexed discovery no longer touches the disk for a tag the tree does not name.
A tag that exists only on disk is still found by the walk channel. Installation
directories remain excluded. Junk, disk and search scans no longer sort every
directory for a ranking the GUI does not show.

## Verification

- `internal/core/devscan_test.go::TestIndexedDiscoveryReadsCacheDirTagOnlyWhenTreeNamesIt`

Proved: before the name check, the test failed at
`indexed discovery opened CACHEDIR.TAG that the tree does not name`;
see `docs/agent-notes-evidence/2026-10-24-indexed-cachedir-name-red.log`.
The same test passes after the check; see
`docs/agent-notes-evidence/2026-10-24-indexed-cachedir-name-green.log`.
