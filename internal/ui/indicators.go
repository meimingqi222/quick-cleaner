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

// indicatorNow returns the paint time and asks for the next frame at the
// display's rate. Loading indicators always animate, whatever the
// desktop's reduce-motion setting says: a static or stepped spinner makes
// a quiet scan read as a hang, and the Rust reference animates
// unconditionally too.
func indicatorNow(p *ui.Painter) time.Time {
	p.AnimationFrame()
	return p.Now()
}

// scanBar is the 3px indeterminate progress bar under the top bar.
func scanBar(c *ui.Context) ui.Element {
	return ui.Box(c).FillWidth().Height(3).Shrink(0).Clip().Background(surfaceHighest).Role(ui.RoleProgress).Draw(func(p *ui.Painter, r ui.Rect) {
		tail, head := scanBarSpan(cycle(indicatorNow(p), scanBarPeriod))
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
	return ui.Box(c).Size(size, size).Shrink(0).Role(ui.RoleProgress).Draw(func(p *ui.Painter, r ui.Rect) {
		width := max(2, r.W*0.11)
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		radius := min(r.W, r.H)/2 - width/2
		var ring ui.Path
		p.StrokePath(ring.Circle(cx, cy, radius), width, primary.Alpha(0.14))
		start, length := spinnerArc(indicatorNow(p))
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
	return ui.Box(c).Absolute().Top(0).Left(0).Size(size, size).PassThrough().Draw(func(p *ui.Painter, r ui.Rect) {
		dx, dy := orbitPosition(indicatorNow(p), radius)
		cx, cy := r.X+r.W/2+dx, r.Y+r.H/2+dy
		p.Fill(ui.Rect{X: cx - dot/2, Y: cy - dot/2, W: dot, H: dot}, color, dot/2)
	})
}

// orbitDotSpecs returns opacity and diameter for the i-th dot (count=8) at phase.
func orbitDotSpecs(phase, i, count int) (opacity float32, dot float32) {
	shift := (phase + (count - i)) % count
	switch shift {
	case 0:
		opacity = 1.0
	case 1:
		opacity = 0.85
	case 2:
		opacity = 0.65
	case 3:
		opacity = 0.45
	case 4:
		opacity = 0.30
	case 5:
		opacity = 0.20
	case 6:
		opacity = 0.12
	default:
		opacity = 0.06
	}
	if shift <= 1 {
		dot = 8.0
	} else if shift <= 3 {
		dot = 7.0
	} else {
		dot = 6.0
	}
	return opacity, dot
}

// orbitSpinner renders an 8-dot rotating orbit with a breathing centre glow,
// faithfully transcribing Rust's LoadingState spinner.
func orbitSpinner(c *ui.Context, size float32) ui.Element {
	return ui.Box(c).Size(size, size).Shrink(0).Role(ui.RoleProgress).Draw(func(p *ui.Painter, r ui.Rect) {
		const count = 8
		radius := (r.W - 12.0) / 2.0
		cx, cy := r.X+r.W/2, r.Y+r.H/2

		animPhase := int(indicatorNow(p).UnixNano() / int64(100*time.Millisecond))
		phase := animPhase % count
		pulse := float32(0.12 + 0.10*math.Sin(float64(animPhase%12)/12.0*math.Pi))
		centreSize := float32(28.0) * (r.W / 64.0)
		p.Fill(ui.Rect{X: cx - centreSize/2, Y: cy - centreSize/2, W: centreSize, H: centreSize}, primary.Alpha(pulse), centreSize/2)

		for i := 0; i < count; i++ {
			angle := float64(i) * (2.0 * math.Pi / float64(count))
			x := cx + radius*float32(math.Cos(angle))
			y := cy + radius*float32(math.Sin(angle))
			opacity, dot := orbitDotSpecs(phase, i, count)
			scaledDot := dot * (r.W / 64.0)
			p.Fill(ui.Rect{X: x - scaledDot/2, Y: y - scaledDot/2, W: scaledDot, H: scaledDot}, primary.Alpha(opacity), scaledDot/2)
		}
	})
}

func loadingDots(now time.Time) string {
	cycle := int(now.UnixNano()/int64(350*time.Millisecond)) % 4
	switch cycle {
	case 1:
		return ".  "
	case 2:
		return ".. "
	case 3:
		return "..."
	default:
		return "   "
	}
}

// loadingState provides a centered, standard loading view with the 72px orbit spinner,
// 20px primary bold title with animated trailing dots, and 13px description.
func loadingState(c *ui.Context, title, sub string) ui.Element {
	dots := loadingDots(c.Now())
	return ui.Column(c).FillWidth().Padding(56, 20).Gap(18).AlignItems(ui.Center).Children(func() {
		orbitSpinner(c, 72)
		ui.Text(c, title+dots).FontSize(20).Bold().TextColor(primary)
		if sub != "" {
			ui.Text(c, sub).FontSize(13).FixedLineHeight(18).TextColor(muted)
		}
	})
}
