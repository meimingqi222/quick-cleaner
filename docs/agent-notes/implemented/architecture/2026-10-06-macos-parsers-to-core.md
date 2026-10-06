# Agent Note: macOS output parsers moved to core for Windows-testable coverage

Status: implemented

## Problem

Three pure string parsers drove the macOS residual/occupancy scanners:

- `parse_launchd_registered` — parses `launchctl print gui/<uid>` output (the `services`
  summary section and the trailing `disabled services` section) into registered,
  not-disabled labels.
- `parse_application_groups` — extracts signed App Groups from `codesign -d --entitlements`
  output.
- `parse_system_extensions` — extracts active system extensions from
  `systemextensionsctl list` output.

They lived in `src/platform/macos/residuals.rs`, whose whole module is
`#[cfg(target_os = "macos")]`. Their logic has nothing macOS-specific — it is pure
`&str` handling — but because of the module gate their unit tests never compiled or ran
on Windows, so a bug in them would not be caught by any Windows gate and would only
surface on a macOS host.

## Decision

Moved the three parsers (plus their helpers `quoted_label`, `contains_ignore_ascii_case`,
`valid_bundle_id`) into a new `src/core/macos_text.rs`, and moved their unit tests with
them. The module is gated `#[cfg(any(test, target_os = "macos"))]`:

- on macOS it compiles for the platform scanners;
- in test builds it compiles on every host, so the parser tests run on Windows;
- on Windows **non-test** builds it is absent, so it does not raise `dead_code` (the
  macOS-only callers are not compiled there).

`platform/macos/residuals.rs` now imports the parsers from `core::macos_text` and keeps
only its platform-specific logic (paths, `codesign`/`launchctl`/`systemextensionsctl`
invocation, item construction). The moved test bodies are deleted from the residuals
test module.

## Alternatives considered

Leaving the parsers macOS-gated keeps portable logic untested on Windows. Gating the
whole `core::macos_text` module to `target_os = "macos"` would defeat the purpose.
`#[cfg(test)]`-only would break the macOS production build. The `any(test, target_os)`
gate is the same pattern already used for `categories`/`status`/`browser` macOS-logic.

## Consequences

The three parser tests (`parses_activated_system_extensions_only`,
`parses_only_signed_application_group_values`,
`launchd_parse_covers_services_section_and_ignores_disabled`) now run in the Windows
suite — the macOS residual/occupancy parsers get real coverage on this host. This is
automated coverage, not macOS native acceptance.

## Verification

- `src/core/macos_text.rs::parses_activated_system_extensions_only`
- `src/core/macos_text.rs::parses_only_signed_application_group_values`
- `src/core/macos_text.rs::launchd_parse_covers_services_section_and_ignores_disabled`

Refactor + test relocation, not an organic bug fix; no red-run proof claimed. Actual
unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
