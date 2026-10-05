# Agent Note: Unquoted UninstallString must not resolve to a directory

Status: implemented

## Problem

Windows registry `UninstallString` values are frequently unquoted. Splitting on spaces from the left and accepting the first hit with `Path::exists()` matches the real directory prefix first: `C:\Program Files (x86)\pdfcvt\uninstall.exe` hits the existing folder `C:\Program Files`, `CreateProcess` fails with access denied (os error 5), and the official uninstaller never appears. The failure looks like a broken vendor uninstaller.

## Decision

The production path of `split_command` accepts only the first prefix that `Path::is_file()` — a directory is never a hit. Never split on the first space alone. Every new parser of `UninstallString` must be exercised against an unquoted path with an intermediate directory prefix such as `C:\Program Files (x86)\…\uninstall.exe`.

## Alternatives considered

`exists()` matches directories and is the bug. Heuristic quoting is unreliable because the registry data itself is inconsistent. Asking the user to reinstall the vendor app fixes nothing.

## Consequences

Directory-prefixed unquoted strings resolve slower (several stat calls) but correctly. Any refactor that touches uninstall launch or command parsing must keep the `is_file()` guard; weakening it back to `exists()` reintroduces the incident silently.

## Verification

- `src/core/apps.rs::directory_prefix_must_not_win_over_the_real_exe`
- `src/core/apps.rs::unquoted_program_files_x86_is_not_a_directory`

Proved: organic red — the motivating failure was observed on the real machine (CreateProcess on the directory prefix returned os error 5 and the uninstall window never appeared, recorded in the pitfalls list before this note existed); the bound tests reproduce the unquoted-parsing failure mode in isolation and pass with the `is_file()` guard in place.
