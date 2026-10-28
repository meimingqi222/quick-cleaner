package ui

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// The scan ring's orbit dot used to read c.Now() and call c.After(1/60s)
// while building, which rebuilt the whole overview 60 times a second.
func TestScanningOverviewAnimatesWithoutRebuilding(t *testing.T) {
	builds := 0
	s := Dashboard{Language: Chinese, Scanning: true}
	tt := ui.NewTester(func(c *ui.Context) { builds++; s.Render(c, DashboardActions{}) }, 1280, 880)
	tt.Frame()
	start := builds
	tt.Advance(time.Second)
	if n := builds - start; n > 2 {
		t.Fatalf("the scanning overview rebuilt %d times in a second", n)
	}
	if !tt.FramePending() {
		t.Fatal("the scan animation stopped asking for frames")
	}
}
