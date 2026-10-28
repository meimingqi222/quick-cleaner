# Agent Note: Go update archive keys preserve Rust Unicode lowercasing

Status: implemented

## Problem

During updater migration validation, Go's simple rune lowercasing both missed a
collision Rust rejects (`ΟΣ` versus `ος`) and invented one Rust permits
(`İ.txt` versus `i.txt`). Archive prevalidation must preserve the reference's
whole-string Unicode lowercasing, not just its ASCII examples.

## Decision

`internal/core/updater_archive.go` uses the existing x/text Unicode casing library
for archive keys and top-level symlink-boundary comparison. The regression checks
both rejected collisions before any writes and distinct names with intact data.

## Alternatives considered

Go strings.ToLower is not equivalent to Rust str::to_lowercase: contextual final
sigma and one-to-many mappings differ. ASCII-only casing would further weaken
the original collision contract. Locale-specific casing and case folding would
change that contract rather than port it.

## Consequences

Duplicate entries, file/directory conflicts and symlink lookup share the same
key function. Unicode tables still follow the pinned Go/x-text toolchain rather
than promising equivalence for newly introduced Unicode characters in every
future Rust release. No update is downloaded from or installed on a real host.

## Verification

- `internal/core/updater_test.go::TestUpdaterArchiveUnicodeCollisionParity`

Proved: manufactured red — after checking the expected mappings with rustc
1.99.1's unchanged str::to_lowercase, replacing only the archive key's casing
operation with strings.ToLower made both public extraction cases fail; see
`docs/agent-notes-evidence/2026-10-23-go-update-unicode-red.log`. The same focused
test passes immediately after restoring full Unicode casing; see
`docs/agent-notes-evidence/2026-10-23-go-update-unicode-green.log`.
