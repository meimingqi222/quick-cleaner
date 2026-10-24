# Agent Note: Go discovered residual cleanup retains live ownership gates

Status: implemented

## Problem

The Go product initially refused all discovered-app residual disposal because
registration absence cannot prove ownership of a portable executable's parent.
Enabling it with the registered-app absence gate would lose the Rust source
installation's exact-path and live shared-owner protections (P14/P16/P37).

## Decision

`ValidateDiscoveredResidualClean` keeps one public gate. Source-layout
discoveries still go through `internal/core/discovered_residual.go`: frozen
rule, snapshot generation and installation scope, live adapter reinspection,
and every selected installation path exactly owned by both the scan and live
adapter. Process inventory must be known and idle.

No-rule, no-plan discoveries are not a blanket refusal. They dispatch to
`validatePortableResidualClean` in `internal/core/discovered_residual_portable.go`.
Portable authority is only the captured executable and captured `.lnk` files
whose targets still resolve to that executable, under
`discovered:<normalized executable>`, one exact executable `ProgramPath`,
frozen `residual-windows` snapshots, matching regular-file identity (Windows
requires stable object identity), and a known process inventory with that
executable not running. Parents, descendants, name-matched data, registry
resources, and mixed evidence refuse. This matches P14: a shortcut proves
launch, not ownership of the parent directory.

`internal/core/discovered_residual_windows.go` supplies the dispatch and native
observations. `internal/app/native_windows.go` invokes the public gate with
exactly the selected items frozen by `internal/app/residuals.go` after explicit
confirmation.

## Alternatives considered

ARP absence, executable-parent inference and successful uninstall exit codes
cannot authorize discovered paths. Turning per-item authority into global app
absence would permit unreviewed paths; rebuilding target scope at acceptance
could silently expand it. Continuing to refuse all discoveries is safe but
omits the original source-layout workflow.

## Consequences

Live evidence can shrink ownership, never expand the reviewed selection.
Portable discoveries are authorized only for the captured executable and
captured shortcuts that still point at it. Missing adapters, unknown process
images for the executable name, redirected paths, and any non-file evidence
remain refused. Batch authorization does not replace deletion-time identity,
rule, path-safety and occupancy gates. The unavoidable native process-check
race is not claimed eliminated. No real installation, registry or user files
were modified during verification.

## Verification

- `internal/core/discovered_residual_test.go::TestDiscoveredResidualAuthority`
- `internal/core/discovered_residual_test.go::TestDiscoveredResidualUnknownAndCancelled`
- `internal/core/discovered_residual_portable_test.go::TestPortableResidualAuthority`
- `internal/app/integration_test.go::TestResidualCleanProbesOnlyAfterTheUserStartsIt`

Proved: manufactured red — removing only the live ProgramPaths membership check
made the focused authority test fail with `late shared owner accepted`, using a
disposable source-install fixture; see
`docs/agent-notes-evidence/2026-10-23-go-discovered-residual-red.log`. Restoring
the guard immediately passes the identical focused command; see
`docs/agent-notes-evidence/2026-10-23-go-discovered-residual-green.log`.
