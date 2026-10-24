# Go / MyGo migration

## Delivery state

The repository now contains a Go product entry point, core/platform ports and
MyGo pages. This is no longer only a dashboard prototype. **Full Rust feature,
visual and performance parity is not established.** The Rust source remains the
behavior reference and the release workflow still builds Rust. Nothing in this
migration changes the published packages or authorizes deleting the reference.

Preserve the existing product, not a redesign: navigation, layout, colors,
icons, copy, themes, localization, selection and confirmation behavior come
from `src/ui/` and the existing assets. Native menus and platform font rendering
are not claimed pixel-identical to GPUI.

## Build and run

Install Go 1.27.1 or newer, required by the pinned MyGo revision.

```sh
go mod download
go test ./...
go vet ./...
go build ./...
go run ./cmd/quick-cleaner --no-elevate
```

`cmd/quick-cleaner` is the real application: its confirmation-gated actions can
modify files, run uninstallers and invoke native helpers. `--no-elevate` only
suppresses automatic Windows elevation; it is not a dry-run flag.
Windows/macOS are the supported product targets. Linux runs the UI and
read-only monitoring, but deliberately refuses unsupported native operations.

For non-destructive fixture previews instead:

```sh
go run ./cmd/ui-gallery --state=found --en
go run ./cmd/ui-gallery --page=junk --state=found --details --en
go run ./cmd/ui-gallery --page=apps --state=found --dev --en
go run ./cmd/ui-gallery --page=status --state=found --processes --details --en
go run ./cmd/ui-gallery --page=search --state=found --en
go run ./cmd/ui-gallery --page=disk --state=found --files --en
go run ./cmd/ui-gallery --page=declutter --state=found --tab=photos --en
go run ./cmd/ui-gallery --page=tray --state=found --width=360 --height=560 --en
go run ./cmd/ui-gallery --state=found --dialog=permission --macos
go run ./cmd/ui-gallery --page=search --state=found --output=search.png
```

Gallery states: `idle`, `scanning`, `empty`, `found`, `cleaning`. Declutter tabs:
`overview`, `photos`, `duplicates`, `large`, `downloads`. Dialog fixtures:
`confirm`, `release`, `uninstall`, `permission`, `update`, `downloading`, `ready`,
`error`. `--macos` is a presentation fixture, not a permission probe. PNG output
works without a display. Gallery business actions are deliberately disconnected.

## Ownership and migrated contracts

| Layer | Implementation |
| --- | --- |
| Product | `cmd/quick-cleaner`, `internal/app`: MyGo startup, main-thread publication, serialized/cancellable work, navigation, selection, stable single-use confirmations, settings/history, native adapters, update/FDA/fan-helper and tray lifecycle integration |
| Presentation | `internal/ui`: shared shell/dashboard, Smart Clean, Apps/residual review, developer master/detail, hardware/processes, Search, Disk Lens, all five Declutter tabs, confirmation/update/permission overlays and tray content; Chinese/English |
| Core | `internal/core`: safety/whitelist, settings/history, typed rules and frozen plans, scans/providers, verified cleanup, apps/uninstall/residuals, developer environments, declutter, disk/query/selection/index caches, monitoring and updates |
| Platform | `internal/platform`: original-user/elevation context, native trash/access, registry/process/task/service adapters, NTFS MFT, macOS bulk traversal/FSEvents, native icons, inventory, thermal/GPU/fan-helper, permissions and installer adapters |
| Resources | `assets/embed.go`, `rules/embed.go`: existing product assets and unchanged bundled declarations |

Views receive snapshots and emit events. They do not enumerate files, authorize
deletion, terminate processes or own window lifecycle. Controllers capture
identities and selection scope before confirmation; the core revalidates at
execution. Unknown observations must never become absence evidence.

The port retains centralized path protection, no-follow cleanup, frozen
rule/identity/owner plans, live SQLite-family gates, per-target completion proof,
official uninstall without destructive fallback, residual revalidation, and
verified ecosystem removal. Docker untagging does not invent released layer
bytes; APFS snapshots do not invent reclaimable bytes. Existing rule fixtures
are compared without regenerating expected baselines.

## Known parity gaps and acceptance gates

- **Native runtime:** Windows/macOS APIs and UI are cross-compiled, not accepted
  on those systems. UAC/desktop-user routing, native menus/IME/accessibility,
  tray focus/reopen/quit, registry/tasks/services, real uninstall/Trash, APFS/MFT,
  FSEvents, FDA, privileged helpers and updater restart need native acceptance.
  The user deferred that acceptance; missing native runners is not a reason to
  stop source migration or weaken guards.
- **Index scalability:** opening a macOS v7 base now maps the file instead of
  reading a second full copy before decode. The live `SizeTree` is still
  heap-owned: Rust spill collection, streamed construction, override-only
  mutation and page discard are not preserved. Incremental persistence still
  diffs the base in O(base-size) work/memory. Native blocked syscalls cannot
  be forcibly interrupted.
- **NTFS accounting:** MFT acquisition is sequential and record-count bounded.
  Extra hard-link parents recorded at scan time are reapplied by
  `RecomputeAggregates`; they are still not separate navigable names.
  Hard links in macOS traversal are counted per path; file sizes use
  allocated bytes. Generic directory inventory instead reports logical bytes,
  not reclaimable space.
- **Windows cross-account residual disposal:** HKCU/recycling transport across
  elevated versus desktop-user accounts is not implemented; those cases refuse
  execution rather than using the administrator's profile or Recycle Bin.
- **Cache interoperability:** v7/v1 payloads retain Rust's layout. Go adds a
  checksummed, generation-bound coverage sidecar published before the index.
  Missing/corrupt coverage fails closed; legacy indexes have unknown coverage.
  Legacy Rust can decode payloads but does not enforce this coverage extension.
  Executable Rust↔Go cache interoperability is not runtime-verified.
- **Presentation:** native MyGo menus replace some bespoke GPUI menus. Disk
  Lens uses the original 24px pane gap; the outer 520px floor is not pinned
  because the loaded page also has a 20px title, optional status line, and
  selected-item bar, and a floor on the row alone would overflow a short
  window. Residual review shows the scan-time process/launchd occupancy
  banner. Mutation cleanup shows the docked file/byte progress bar and Stop
  control. macOS FDA settings now also shows the drag-to-authorize panel when
  the process is inside an .app bundle; it is not accepted on a Mac in this
  checkout. Windows elevation remains a startup relaunch, not a clickable
  title badge.
- **Image algorithms:** native icon/image codecs and triangle filtering are
  not claimed byte-identical. Duplicate detection intentionally preserves Rust's
  five-region sampling above 2 MiB, not full-file equality. Stable Go candidate
  order can change ambiguous/capped photo grouping relative to Rust HashMap
  iteration. Native decoder/ARM64 arithmetic parity remains unverified.
- **Packaging:** signing, Go bundles/installers and release-workflow switchover
  remain undone. The Go updater must not be assumed compatible with the current
  Rust release artifacts without native package validation.

Do not call this a complete Rust replacement while these gates remain. Do not
claim improved build speed from comparing a partial port with the full product;
no controlled before/after comparison has been established.

## Verification scope

Go tests exercise real isolated temporary-file cleanup, preserved/whitelisted
descendants, links/replacements, live-database refusal, frozen commands, owner
inventories and postconditions. Disposable executable fixtures cover ecosystem
removal, Git worktrees, Docker/Homebrew/snapshots and failed-command refusal.
Tests do not mutate real developer installations, native Trash or shared state.

Index tests cover malformed binary input, allocated-byte and parent accounting,
partial/cancelled acquisition, bounded workers, replay, durable unknown coverage,
atomic publication/rollback and guarded orphan pruning. Platform-native test
binaries can compile on Linux without proving native execution.

Updater tests use local HTTP servers, real staging/archive operations and
temporary bundle rollback. SHA-256 sidecars establish integrity, not publisher
authentication. Unicode archive collision regression evidence is retained in
`docs/agent-notes-evidence/`. No production update or privilege prompt was run.

MyGo public input tests cover stable callbacks, filtering, sorting, selection,
busy gates, virtualization, modal focus and immutable snapshots. Offscreen and
native GTK/X11 gallery renders cover both languages and representative page,
loading/error/partial/dialog states. Native Search input checks exercise wildcard
filtering, clearing, grouping/sorting, unmatched queries and Escape. Photo-page
scrolling reaches lower cards and expansion controls. These checks are not
Windows/macOS visual acceptance or proof of destructive native behavior.

The orb restricts some `/proc/*/fd` observations. Linux occupancy can therefore
remain unknown for a closed file; stale SQLite disposal correctly refuses it.
Positive native stale-family cleanup remains unverified. Existing regression
notes and `docs/PITFALLS.md` remain applicable to corresponding Go behavior.
