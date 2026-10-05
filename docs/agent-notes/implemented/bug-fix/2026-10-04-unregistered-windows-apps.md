# Agent Note: Discover unregistered Windows apps without inventing uninstall authority

Status: implemented

## Problem

Hermes is installed as source and unpacked desktop files without an ARP entry, so registry-only enumeration cannot show it. Treating every shortcut parent as an installation directory would also authorize deletion of unrelated downloads and tools.

## Decision

Discover local executable shortcuts and adapter-supplied default layouts. Preserve exact launch evidence, deduplicate by canonical path, and let registered installations win by path. Unknown portable apps own only their executable and verified shortcuts. Official uninstall requires a content/layout adapter, revalidated immediately before execution. Hermes uses its external Python runtime and official lite mode, preserving user data. Completion requires all installation artifacts gone and processes settled; exit zero and missing ARP do not prove success.

## Alternatives considered

Adding a Hermes-only list item would leave other unpacked applications invisible. Executing a guessed uninstall script or recursively removing every exe parent cannot establish ownership. Running Python from the source tree would lock files the official uninstaller removes on Windows.

## Consequences

Unregistered apps are visible with explicit capabilities. Unknown layouts have conservative file removal, while new official providers need an adapter and verification. Display-name matches never preselect data removal. Shell-free argv and the real user's token/environment preserve execution identity.

## Verification

- `src/platform/windows/app_discovery.rs::hermes_without_arp_is_discovered_with_official_uninstall`
- `src/platform/windows/app_discovery.rs::portable_shortcuts_do_not_claim_parent_directory_or_uninstall_authority`
- `src/platform/windows/app_discovery.rs::discovered_user_data_and_name_only_shortcuts_are_never_preselected`
- `src/platform/windows/app_discovery.rs::official_uninstall_process_preserves_data_and_completes_noop_with_verified_fallback`

Proved: temporarily replacing the adapter table with an empty table made the Hermes test fail (expected one combined application, got two; cargo exit 101). Restoring the adapter passed that exact test. The isolated official-process test exercises preserved-data removal and a no-op exit-zero process followed by verified owned-artifact cleanup. Exit zero alone remains insufficient. Full integrated discovery found the real local Hermes without invoking its uninstaller.
