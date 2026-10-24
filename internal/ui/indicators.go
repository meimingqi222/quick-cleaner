package ui

import (
	"math"
	"time"

	"github.com/egoist/mygo/ui"
)

// Loading indicators move in paint-only frames: their Draw callbacks read
// the paint clock and ask for the next frame, so they run at the display's
// rate without rebuilding the page. Reading c.Now() while building the view
// did not ask for frames; a bar built that way moved only when something
// else repainted, and stood still through a quiet MFT parse.

const (
	scanBarPeriod = 1500 * time.Millisecond
	spinnerCycle  = 1333 * time.Millisecond
	spinnerTurn   = 1568 * time.Millisecond
)

// cycle returns how far now is through a period, in [0, 1).
func cycle(now time.Time, period time.Duration) float64 {
	return float64(now.UnixNano()%int64(period)) / float64(period)
}

func easeInQuad(t float64) float64  { return t * t }
func easeOutQuad(t float64) float64 { return t * (2 - t) }
func easeInOutCubic(t float64) float64 {
	if t < 0.5 {
		return 4 * t * t * t
	}
	return 1 - math.Pow(-2*t+2, 3)/2
}

// scanBarSpan returns the indeterminate bar's tail and head as fractions of
// the track at phase t. The head eases out and the tail eases in, so the bar
// stretches as it leaves the left edge and shortens as it reaches the right.
func scanBarSpan(t float64) (tail, head float64) {
	return 1.2*easeInQuad(t) - 0.1, 1.2*easeOutQuad(t) - 0.1
}

// scanBar is the 3px indeterminate progress bar under the top bar.
func scanBar(c *ui.Context) ui.Element {
	still := c.Preferences().ReduceMotion
	return ui.Box(c).FillWidth().Height(3).Shrink(0).Clip().Background(surfaceHighest).Role(ui.RoleProgress).Draw(func(p *ui.Painter, r ui.Rect) {
		tail, head := 0.0, 0.35
		if !still {
			tail, head = scanBarSpan(cycle(p.Now(), scanBarPeriod))
			p.AnimationFrame()
		}
		x0 := r.X + r.W*float32(max(0, tail))
		x1 := r.X + r.W*float32(min(1, head))
		if x1 > x0 {
			p.Fill(ui.Rect{X: x0, Y: r.Y, W: x1 - x0, H: r.H}, primary, r.H/2)
		}
	})
}

// spinnerArc returns the arc's start and length as fractions of a turn at
// time now: the arc grows to three quarters and shrinks back while the
// whole turns, each cycle starting where the last one's tail stopped.
func spinnerArc(now time.Time) (start, length float64) {
	n := now.UnixNano()
	k := n / int64(spinnerCycle)
	u := cycle(now, spinnerCycle)
	head := 0.75 * easeInOutCubic(min(1, u/0.5))
	tail := 0.75 * easeInOutCubic(max(0, (u-0.5)/0.5))
	base := cycle(now, spinnerTurn) + 0.75*float64(k%4)
	start = base + tail
	return start - math.Floor(start), head - tail + 0.03
}

// loadingSpinner is a rotating arc on a faint ring, in the primary color.
func loadingSpinner(c *ui.Context, size float32) ui.Element {
	still := c.Preferences().ReduceMotion
	return ui.Box(c).Size(size, size).Shrink(0).Role(ui.RoleProgress).Draw(func(p *ui.Painter, r ui.Rect) {
		width := max(2, r.W*0.11)
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		radius := min(r.W, r.H)/2 - width/2
		var ring ui.Path
		p.StrokePath(ring.Circle(cx, cy, radius), width, primary.Alpha(0.14))
		start, length := 0.0, 0.7
		if !still {
			start, length = spinnerArc(p.Now())
			p.AnimationFrame()
		}
		// Circle starts at three o'clock and runs clockwise; an arc past the
		// end of the circle is drawn as two trims.
		end := start + length
		var arc ui.Path
		p.StrokePath(arc.Circle(cx, cy, radius).Trim(float32(start), float32(min(end, 1))), width, primary)
		if end > 1 {
			var wrap ui.Path
			p.StrokePath(wrap.Circle(cx, cy, radius).Trim(0, float32(end-1)), width, primary)
		}
	})
}

const orbitPeriod = 1500 * time.Millisecond

// orbitPosition returns the dot's centre offset from the ring's centre at
// time now, starting at twelve o'clock and running clockwise.
func orbitPosition(now time.Time, radius float32) (float32, float32) {
	a := cycle(now, orbitPeriod) * 2 * math.Pi
	return radius * float32(math.Sin(a)), -radius * float32(math.Cos(a))
}

// orbitDot overlays a size×size box with a dot circling its centre at
// radius, for the overview's scan ring.
func orbitDot(c *ui.Context, size, radius, dot float32, color ui.Color) ui.Element {
	still := c.Preferences().ReduceMotion
	return ui.Box(c).Absolute().Top(0).Left(0).Size(size, size).PassThrough().Draw(func(p *ui.Painter, r ui.Rect) {
		dx, dy := float32(0), -radius
		if !still {
			dx, dy = orbitPosition(p.Now(), radius)
			p.AnimationFrame()
		}
		cx, cy := r.X+r.W/2+dx, r.Y+r.H/2+dy
		p.Fill(ui.Rect{X: cx - dot/2, Y: cy - dot/2, W: dot, H: dot}, color, dot/2)
	})
}
