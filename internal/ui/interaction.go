package ui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// hoverFade eases a control's background between its rest, hover and press
// colours. It is short enough that a sweep across a list does not trail.
var hoverFade = ui.Animation{Duration: 120 * time.Millisecond}

// pressScale is how far a pressed control dips. The dip is a pose, which
// MyGo moves in paint-only frames without rebuilding the page.
const pressScale = 0.97

// interactive gives b its background with eased hover and press feedback,
// and a slight dip while pressed. A disabled control stays at rest.
func interactive(b ui.Element, rest, hover, press ui.Color, enabled bool) ui.Element {
	target := rest
	pressed := enabled && b.Pressed()
	switch {
	case pressed:
		target = press
	case enabled && b.Hovered():
		target = hover
	}
	b.Background(b.AnimateColor("qc-background", target, hoverFade))
	scale := float32(1)
	if pressed {
		scale = pressScale
	}
	return b.AnimatePose(ui.Pose{Scale: scale}, ui.Snappy)
}

// interactiveRow eases a list row's hover background. Rows do not dip: a
// whole row scaling reads as the list moving.
func interactiveRow(row ui.Element, rest, hover ui.Color) ui.Element {
	target := rest
	if row.Hovered() {
		target = hover
	}
	return row.Background(row.AnimateColor("qc-row", target, hoverFade))
}

// chevron draws a disclosure arrow pointing right, turned down while open.
// The turn is a pose, so it animates in paint-only frames.
func chevron(c *ui.Context, open bool, color ui.Color, size float32) ui.Element {
	angle := float32(0)
	if open {
		angle = 90
	}
	return ui.Box(c).Size(size, size).Shrink(0).PassThrough().Draw(func(p *ui.Painter, r ui.Rect) {
		cx, cy, d := r.X+r.W/2, r.Y+r.H/2, r.W*0.2
		var path ui.Path
		path.MoveTo(cx-d*0.6, cy-d*1.25).LineTo(cx+d*0.7, cy).LineTo(cx-d*0.6, cy+d*1.25)
		p.StrokePath(&path, max(1.5, r.W*0.12), color)
	}).AnimatePose(ui.Pose{Rotate: angle}, ui.Snappy)
}

// disclosure is how a revealed block appears and leaves under its header.
var disclosure = ui.ElementTransition{Duration: 160 * time.Millisecond, Enter: &ui.Motion{Opacity: 0, Y: -6}, Exit: &ui.Motion{Opacity: 0}}
