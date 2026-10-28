# Agent Note: MFT record count follows the allocated runs, not a fixed cap

Status: implemented

## Problem

`mftScanReader` rejected any volume whose valid length held more than 16M
records with `ErrMFTCorrupt`. A large NTFS volume therefore failed the whole
scan. Rust has no such cap: it reads until the smaller of the valid length and
the allocated runs. The cap existed to stop a corrupt valid length from
forcing one huge upfront allocation.

## Decision

In `internal/platform/mft_scan.go`, the record count is
`min(valid length, run bytes) / record size`. A count beyond uint32 is still
corrupt, because tree indexes are uint32. `Entries` grows per 8 MiB chunk
instead of being allocated up front. A corrupt run list therefore fails on
its first unreadable chunk before it allocates its claimed size.

## Alternatives considered

- Raise the fixed cap. Rejected: any fixed number is wrong for some volume.
- Keep the upfront allocation sized by the runs. Rejected: corrupt runs could
  still claim terabytes before a single read.

## Consequences

Large volumes scan. A valid length beyond the runs reads what the runs hold,
as Rust does, instead of erroring at the tail.

## Verification

- `internal/platform/mft_test.go::TestMFTRecordCountFollowsRunsNotAFixedCap`

Proved: with the fixed 16M cap restored, the test failed at
`valid length beyond a fixed cap rejected: invalid NTFS MFT metadata`; see
`docs/agent-notes-evidence/2026-10-24-mft-record-cap-red.log`. With the
run-bounded count, the same test passes; see
`docs/agent-notes-evidence/2026-10-24-mft-record-cap-green.log`.
