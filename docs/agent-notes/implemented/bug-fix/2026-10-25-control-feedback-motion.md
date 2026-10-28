# Agent Note: Controls give eased hover, press and disclosure feedback

Status: implemented

## Problem

No control in the Go UI had a press state. Hover colours switched instantly,
dialogs popped in at full size, and categories and process groups expanded
behind text glyphs (▸ ▾ › ⌄). The failure banner was a solid red slab.

## Decision

`internal/ui/interaction.go`:

- `interactive` eases a control's background through rest, hover and press
  colours (`AnimateColor`, 120 ms). It dips the control to 97% while pressed
  (`AnimatePose`, which repaints without a rebuild). Disabled controls stay
  at rest.
- `interactiveRow` eases list-row hovers without the dip.
- `chevron` is a drawn disclosure arrow that turns 90° as a pose.
- `disclosure` fades revealed blocks in from 6px above.

The shared buttons, sidebar, overview cards and circle, tray controls,
process and other list rows, and the clean button use these. Dialogs fade
and grow from 96% with `Keyframes`, because the overlay is new on every open
and an Enter transition has no previous frame. The failure banner is now a
light tint with a red heading and body-text details.

## Alternatives considered

- Spring colour animation. Rejected: springs overshoot colours, and a fixed
  short curve reads better in lists.
- Transitions on every row. Rejected: whole rows scaling reads as the list
  moving.

## Consequences

Hover and press feedback costs a few short frames per change. A settled UI
asks for no frames.

## Verification

- `internal/ui/interaction_test.go::TestButtonsEaseHoverAndPressFeedback`
- `internal/ui/interaction_test.go::TestDialogEntranceAnimatesThenSettles`
- `internal/ui/interaction_test.go::TestCategoryChevronTurnsThenSettles`

Proved: with the instant hover restored, the button test failed at `the
hover colour did not ease; it switched at once`; see
`docs/agent-notes-evidence/2026-10-25-control-feedback-red.log`. With the
dialog keyframes and the chevron removed, the other two failed at `the
dialog appeared without an entrance` and `the category still uses a text
glyph`; see `docs/agent-notes-evidence/2026-10-25-motion-entrance-red.log`.
Restored, all pass; see
`docs/agent-notes-evidence/2026-10-25-control-feedback-green.log` and
`docs/agent-notes-evidence/2026-10-25-motion-entrance-green.log`.
