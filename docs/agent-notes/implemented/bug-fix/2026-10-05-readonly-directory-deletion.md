# Agent Note: Read-only directory bits need their own clear-and-retry on Windows

Status: implemented

## Problem

Go's toolchain marks module directories `FILE_ATTRIBUTE_READONLY` on purpose. `RemoveDirectory` returns `ERROR_ACCESS_DENIED` for a read-only directory — unlike Unix, where a read-only directory still allows deleting its contents and rmdir'ing it when empty. Cleanup logs filled with `拒绝访问 (os error 5)` on directories (`…@v1.2.3`, `.github`, `.circleci`) after files had already been deleted, leaving empty shells whose parents then failed with `目录不是空的 (os error 145)`. Users read this as "the whole Go cache cannot be cleaned".

## Decision

`delete_tree`'s directory exit must go through `remove_dir_forcing`: try the delete, clear the directory read-only bit, retry. The file-side `clear_readonly` retry does not cover this — directory attributes are a separate mechanism. Any new "clear contents, then remove self" path must exit through `remove_dir_forcing`, never a bare `std::fs::remove_dir`.

## Alternatives considered

A single `std::fs::remove_dir` — the bug. Assuming the file-level read-only handling is enough — it is not; the attribute lives on the directory entry itself.

## Consequences

Deletion of read-only directory shells costs one extra attribute-clear round only when the first attempt is denied. The bound test covers a read-only directory containing a file, asserting the whole tree is removed.

## Verification

- `src/core/cleaner.rs::deletes_readonly_directory_shell`

Proved: organic red — the motivating failure was observed on the real machine (os error 5 on Go module directories, os error 145 on their parents; recorded in the pitfalls list before this note existed); the bound test reproduces the read-only directory shell in isolation and passes through `remove_dir_forcing`.
