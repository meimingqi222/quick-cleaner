package ui

import (
	"bytes"
	"image"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// primarySpan returns the first and last x whose pixel in row y is close to
// the primary color, or -1, -1.
func primarySpan(img *image.RGBA, y int) (int, int) {
	first, last := -1, -1
	for x := img.Rect.Min.X; x < img.Rect.Max.X; x++ {
		c := img.RGBAAt(x, y)
		if c.B > 120 && c.R < 80 && c.G < 140 {
			if first < 0 {
				first = x
			}
			last = x
		}
	}
	return first, last
}

func primaryPixels(img *image.RGBA) int {
	n := 0
	for y := img.Rect.Min.Y; y < img.Rect.Max.Y; y++ {
		for x := img.Rect.Min.X; x < img.Rect.Max.X; x++ {
			if c := img.RGBAAt(x, y); c.B > 120 && c.R < 80 && c.G < 140 {
				n++
			}
		}
	}
	return n
}

// The bar used to read c.Now() while building, which asks for no frame: it
// moved only when something else repainted, so a quiet scan froze it.
func TestScanBarKeepsAnimatingOnItsOwn(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card).Children(func() { scanBar(c) })
	}, 600, 40)
	tt.Advance(400 * time.Millisecond)
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("the scan bar asked for no next frame; it stands still until something else repaints")
	}
	// The tester's clock starts at an arbitrary phase, and the bar is off
	// the track between cycles, so sample a whole period.
	seen := map[[2]int]bool{}
	for range int(scanBarPeriod / (100 * time.Millisecond)) {
		tt.Advance(100 * time.Millisecond)
		tt.Frame()
		if a, b := primarySpan(tt.Image(), 1); a >= 0 {
			seen[[2]int{a, b}] = true
		}
	}
	if len(seen) < 3 {
		t.Fatalf("the bar showed %d distinct positions over a period", len(seen))
	}
}

func TestLoadingSpinnerKeepsAnimatingOnItsOwn(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card).Children(func() { loadingSpinner(c, 28) })
	}, 40, 40)
	tt.Advance(200 * time.Millisecond)
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("the spinner asked for no next frame")
	}
	before := tt.Image()
	snapshot := image.NewRGBA(before.Rect)
	copy(snapshot.Pix, before.Pix)
	tt.Advance(250 * time.Millisecond)
	tt.Frame()
	after := tt.Image()
	if primaryPixels(snapshot) == 0 {
		t.Fatal("the spinner drew no arc")
	}
	same := true
	for i := range snapshot.Pix {
		if snapshot.Pix[i] != after.Pix[i] {
			same = false
			break
		}
	}
	if same {
		t.Fatal("the spinner did not move")
	}
}

func TestOrbitSpinnerKeepsAnimatingOnItsOwn(t *testing.T) {
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card).Children(func() { orbitSpinner(c, 64) })
	}, 80, 80)
	tt.Advance(200 * time.Millisecond)
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("the orbit spinner asked for no next frame")
	}
	before := tt.Image()
	snapshot := image.NewRGBA(before.Rect)
	copy(snapshot.Pix, before.Pix)
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
	after := tt.Image()
	if primaryPixels(snapshot) == 0 {
		t.Fatal("the orbit spinner drew no pixels")
	}
	same := true
	for i := range snapshot.Pix {
		if snapshot.Pix[i] != after.Pix[i] {
			same = false
			break
		}
	}
	if same {
		t.Fatal("the orbit spinner did not move")
	}
}

// A machine with Windows animations off reports ReduceMotion; indicators
// that then slow down or freeze make a quiet scan read as a hang and feel
// choppy over RDP. Like the Rust reference, they animate at the display's
// rate regardless of the desktop's reduce-motion setting.
func TestIndicatorsAnimateUnderReducedMotion(t *testing.T) {
	cases := []struct {
		name string
		el   func(c *ui.Context) ui.Element
	}{
		{"scanBar", func(c *ui.Context) ui.Element { return scanBar(c) }},
		{"loadingSpinner", func(c *ui.Context) ui.Element { return loadingSpinner(c, 28) }},
		{"orbitSpinner", func(c *ui.Context) ui.Element { return orbitSpinner(c, 64) }},
		{"orbitDot", func(c *ui.Context) ui.Element { return orbitDot(c, 230, 112, 12, primary) }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			tt := ui.NewTester(func(c *ui.Context) {
				ui.Column(c).Fill().Background(card).Children(func() { tc.el(c) })
			}, 240, 240)
			tt.SetPreferences(ui.Preferences{ReduceMotion: true})
			tt.Frame()
			if !tt.FramePending() {
				t.Fatalf("%s asked for no next frame under reduced motion", tc.name)
			}
			first := image.NewRGBA(tt.Image().Rect)
			copy(first.Pix, tt.Image().Pix)
			changed := false
			for i := 0; i < 6 && !changed; i++ {
				tt.Advance(100 * time.Millisecond)
				changed = !bytes.Equal(first.Pix, tt.Image().Pix)
			}
			if !changed {
				t.Fatalf("%s stood still under reduced motion", tc.name)
			}
		})
	}
}

func TestIndicatorGeometryStaysInRange(t *testing.T) {
	for i := 0; i <= 100; i++ {
		tail, head := scanBarSpan(float64(i) / 100)
		if tail > head || tail < -0.1 || head > 1.1 {
			t.Fatalf("bar span at %d%%: %f..%f", i, tail, head)
		}
	}
	if tail, head := scanBarSpan(0); head > 0 || tail > 0 {
		t.Fatal("the bar must enter from the left edge")
	}
	base := time.Unix(0, 0)
	for ms := 0; ms < 4000; ms += 7 {
		start, length := spinnerArc(base.Add(time.Duration(ms) * time.Millisecond))
		if start < 0 || start >= 1 || length < 0.03 || length > 0.78 {
			t.Fatalf("arc at %dms: start %f length %f", ms, start, length)
		}
	}
}
