# Agent Note: OS version badges are shortened at the source and clipped by the card

Status: implemented

## Problem

The health overview card drew its OS version badge outside the card boundary, overlapping the neighboring card on Windows. Two causes stack: `long_os_version()` is platform-dependent ("Windows 11 Home China" with a Chinese SKU is far longer than "macOS 15.6"), so layouts sized against the macOS string overflow on Windows; and gpui's `text_ellipsis` is unreliable — a nowrap text's first measurement (`MaxContent`) is cached, so once a definite width arrives it is neither truncated nor wrapped, it just draws out.

## Decision

Shorten at the source: `short_os_name` keeps only "system + version number" and drops the SKU (`os_name_keeps_version_and_drops_the_sku`). Backstop in layout: the badge renders through `header_chip` (shrinkable + truncate) and the card shell `status_card()` sets `overflow_hidden`, so anything that still draws out is clipped.

## Alternatives considered

Relying on gpui ellipsis alone — broken by the measurement cache. Sizing the layout from the macOS string length — the platform with the longest SKU wins and it is not macOS.

## Consequences

New content pushed into a badge must be eyeballed on a Windows Chinese-language system before shipping; neither the source shortening nor the clip makes long content fit gracefully. `status_card()` owns the box metrics; individual cards must not re-state `flex_1 / min_w / p_5` locally.

## Verification

- `src/core/status.rs::os_name_keeps_version_and_drops_the_sku`

Proved: organic red — the motivating failure was observed on real Windows hardware (badge painted over the adjacent card; macOS did not reproduce it); the bound test pins the source-side shortening and the card-level clip is the layout backstop.
