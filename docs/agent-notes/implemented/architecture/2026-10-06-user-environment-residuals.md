# Agent Note: User environment values join the residual channel with explicit ownership

Status: implemented

## Problem

GOAL C lists 用户环境 (user environment) among the native registration kinds the residual
channel must cover with precise ownership and verified completion. The Windows residual
scanners covered registration entries, Run keys, services, tasks, firewall rules and more,
but nothing scanned `HKCU\Environment`: a variable left behind by an uninstalled app kept
pointing at a vanished install directory, and the unused value stayed forever.

## Decision

`scan_user_environment` reads the `user_environment_keys` anchor (shipped value:
`Environment`) and claims only values with ownership evidence: a value mentioning the
install directory or a known executable name is `Certain`; a variable name close to the
app name is `Possible`; anything else is not claimed. The anchor lives in
`residual-windows.toml` like the other scan anchors, and an empty list disables the
scanner. Only the current user's key is scanned — the system-wide environment under HKLM
affects every account and is deliberately not claimed. The item is a `RegistryValue`, so
it deletes through the verified value path (delete + `reg_value_absent` tri-state check)
and only the value is removed, never the `Environment` key itself, which is a Windows
skeleton key.

The matching rule is factored into `user_env_evidence`, a pure function, because tests
must never write the real environment key — the live key is only ever read
(`enum_string_values`).

## Alternatives considered

Claiming variables by name alone — rejected, name-only evidence is `Possible` everywhere
else in the channel, and env var names are unusually generic (`HOME`, `TOOLS`). Scanning
HKLM system environment — rejected: it affects all accounts and needs admin; a wrong
claim there is not a per-user cleanup. Writing a test that creates real environment
values — rejected: it would mutate the developer's environment; the pure-function test
plus the already-covered verified value deletion give the same guarantees.

## Consequences

An uninstalled app's stale `HKCU\Environment` values are now listed with their ownership
evidence and removable through the verified path. The scanner ships enabled (the anchor
is non-empty in the shipped bundle); clearing the anchor in configuration disables it.
`ResidualSource::UserEnvEntry` labels the items bilingually.

## Verification

- `src/platform/windows/residuals.rs::user_environment_evidence_requires_value_or_name_evidence`
- `src/platform/windows/residuals.rs::user_environment_anchor_follows_the_rule`
- `src/platform/windows/residuals.rs::residual_shared_lists_follow_the_rule`
- `src/platform/windows/residuals.rs::registry_residual_clean_verifies_absence_after_delete`
  (value deletion completion criterion, shared with this channel)

Migration + capability addition, not an organic bug fix; no red-run claimed. Windows-only
evidence; macOS native acceptance remains an external condition. Actual unified gate
results are recorded in RULES_REFACTOR_STATUS.
