# Agent Note: A vendor launcher beside the install dir belongs to the registered app

Status: implemented

## Problem

The apps list showed Qoder and Qoder CN twice: once from the uninstall
registration, and again as unregistered apps of about 115 KB. Their Start
Menu and desktop shortcuts point at
`%LOCALAPPDATA%\Qoder\Qoder Launcher\Qoder Launcher.exe`, a vendor launcher
whose `install.ini` points back at `%LOCALAPPDATA%\Programs\Qoder`. The
launcher is outside the registered install directory, so the subtree check in
`discoverAppCandidates` did not cover it.

## Decision

`internal/core/apps_discovery.go` skips a shortcut candidate when a registered
app with a known install location claims it. Both conditions must hold:

- the shortcut name equals the registered display name without its trailing
  version (`windowsResidualShortName`);
- an ancestor directory of the target has the same name as the install
  directory.

The check runs only for plain shortcut candidates, not for source-adapter
matches. Hiding the candidate grants no deletion. Residual scanning of the
registered app still finds the launcher folder by name.

## Alternatives considered

- Parse `install.ini` of the launcher. Rejected: vendor-specific format.
- Compare Authenticode signers. Rejected for now: no signer helper exists,
  and the same publisher can ship separate products.
- Match the shortcut name alone. Rejected: a portable copy named like a
  registered app would vanish from the list.

## Consequences

The "Qoder" shortcut can never be claimed by "Qoder CN", because the names
must be equal. A launcher in an unrelated folder that only shares a name
stays listed.

## Verification

- `internal/core/apps_discovery_test.go::TestAppDiscoveryVendorLauncherBelongsToRegisteredApp`

Proved: before the fix, the test failed at `vendor launcher listed as a
separate app`, listing both launchers; see
`docs/agent-notes-evidence/2026-10-25-vendor-launcher-dedup-red.log`. After
the fix it passes; see
`docs/agent-notes-evidence/2026-10-25-vendor-launcher-dedup-green.log`.
