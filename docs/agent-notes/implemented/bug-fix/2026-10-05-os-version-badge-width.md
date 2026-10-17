# Agent Note: OS version badges are shortened at the source and clipped by the card

Status: implemented

## Problem

The health overview card drew its OS version badge outside the card boundary, overlapping the neighboring card on Windows. Two causes stack: `long_os_version()` is platform-dependent ("Windows 11 Home China" with a Chinese SKU is far longer than "macOS 15.6"), so layouts sized against the macOS string overflow on Windows; and gpui's `text_ellipsis` is unreliable — a nowrap text's first measurement (`MaxContent`) is cached, so once a definite width arrives it is neither truncated nor wrapped, it just draws out.

2026-10-17 recurrence: the user's Windows screenshot shows the process table's
AppleMobileDeviceService.exe port badges overlapping the CPU column when the
330px detail drawer opens. The outer name column could shrink, but its text had
only a character-count limit and the column did not clip overflowing children.

## Decision

Shorten at the source: `short_os_name` keeps only "system + version number" and drops the SKU (`os_name_keeps_version_and_drops_the_sku`). Backstop in layout: the badge renders through `header_chip` (shrinkable + truncate) and the card shell `status_card()` sets `overflow_hidden`, so anything that still draws out is clipped.

For process rows, allow the name text to shrink to zero and truncate inside its
allocated width; keep the port badge strip and group-count badge non-shrinking.
Clip the name column itself in single, group-header and group-child rows, rather
than relying on text ellipsis to protect the adjacent numeric columns. Existing
port selection (first two plus a remaining count) and numeric widths are unchanged.

## Alternatives considered

Relying on gpui ellipsis alone — broken by the measurement cache. Sizing the layout from the macOS string length — the platform with the longest SKU wins and it is not macOS.

Shorter hard-coded process-name limits still fail at different window widths,
font metrics and drawer states. Clipping the entire table would not prevent
name-column content from drawing over the CPU column inside that same table.

## Consequences

New content pushed into a badge must be eyeballed on a Windows Chinese-language system before shipping; neither the source shortening nor the clip makes long content fit gracefully. `status_card()` owns the box metrics; individual cards must not re-state `flex_1 / min_w / p_5` locally.

## Verification

- `src/core/status.rs::os_name_keeps_version_and_drops_the_sku`
- `src/ui/views/status/process_rows.rs::process_ports_stay_before_cpu_with_detail_open_and_closed`

Proved: organic red — the motivating failure was observed on real Windows hardware (badge painted over the adjacent card; macOS did not reproduce it); the bound test pins the source-side shortening and the card-level clip is the layout backstop.

Proved: the process-table recurrence is an organic visual red supplied by the user;
the source and observed overlap are recorded in
`docs/agent-notes-evidence/2026-10-17-process-ports-overlap-red.log`.
The new GPUI regression mounts the actual table with a single process, an expanded
group and multi-port badges, at 1320px and 1000px, with the drawer open and closed.
It asserts both strips end before the CPU cell begins. Native execution and
post-fix screenshots remain pending: the Linux orb lacks GTK/Pango development
libraries, the product has no Linux platform implementation, and no native runner
is connected. The test has not been claimed green; formatting and Python UI guards
pass. Pitfall P3's clipping backstop is preserved and applied to the column boundary.
