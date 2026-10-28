package ui

import (
	"image/color"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func backgroundAt(tt *ui.Tester, x, y float32) color.RGBA {
	return tt.Image().RGBAAt(int(x), int(y))
}

// Buttons used to switch colour instantly on hover and had no press state.
// Feedback must ease, settle (no frames forever), and differ when pressed.
func TestButtonsEaseHoverAndPressFeedback(t *testing.T) {
	disabled := false
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card).Center().Children(func() {
			appsPageButton(c, "probe", "Probe", disabled, false, func() {})
		})
	}, 240, 120)
	label, ok := tt.Find("Probe")
	if !ok {
		t.Fatal("missing button")
	}
	x, y := label.X-4, label.Y+label.H/2
	rest := backgroundAt(tt, x, y)

	tt.Move(label.X+label.W/2, y)
	tt.Advance(40 * time.Millisecond)
	if !tt.FramePending() {
		t.Fatal("the hover colour did not ease; it switched at once")
	}
	tt.Advance(400 * time.Millisecond)
	hover := backgroundAt(tt, x, y)
	if hover == rest {
		t.Fatal("hover changed nothing")
	}
	if tt.FramePending() {
		t.Fatal("a settled hover keeps asking for frames")
	}

	tt.Press(label.X+label.W/2, y)
	tt.Advance(400 * time.Millisecond)
	if pressed := backgroundAt(tt, x, y); pressed == hover {
		t.Fatal("pressing looks the same as hovering")
	}

	disabled = true
	tt2 := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card).Center().Children(func() {
			appsPageButton(c, "probe", "Probe", true, false, func() {})
		})
	}, 240, 120)
	before := backgroundAt(tt2, x, y)
	tt2.Move(label.X+label.W/2, y)
	tt2.Advance(400 * time.Millisecond)
	if after := backgroundAt(tt2, x, y); after != before {
		t.Fatal("a disabled button reacted to hover")
	}
}

// A dialog used to pop in at full size; it now plays a short entrance and
// then stops asking for frames.
func TestDialogEntranceAnimatesThenSettles(t *testing.T) {
	open := false
	d := ConfirmationDialog{Language: English, ID: "x", Title: core.NewText("确认", "Confirm"), Body: core.NewText("正文", "Body")}
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Background(card)
		if open {
			d.Open = true
			d.Render(c, ConfirmationDialogActions{Cancel: func(string) {}, Accept: func(string) {}})
		}
	}, 800, 600)
	open = true
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("the dialog appeared without an entrance")
	}
	tt.Advance(500 * time.Millisecond)
	if tt.FramePending() {
		t.Fatal("the dialog entrance never settles")
	}
	if !tt.HasText("Confirm") {
		t.Fatal("dialog content missing")
	}
}

// Expanding a category turns its chevron instead of swapping a text glyph.
func TestCategoryChevronTurnsThenSettles(t *testing.T) {
	page := JunkPage{Language: English, Scanned: true, Expanded: map[core.CategoryID]bool{},
		Categories: []core.CategorySummary{{Category: core.UserCache, TotalSize: 1, Items: []core.ScanItem{{Path: "/fixture/a", Size: 1}}}}}
	tt := ui.NewTester(func(c *ui.Context) { page.RenderContent(c, JunkPageActions{}) }, 1280, 800)
	tt.Advance(time.Second)
	if tt.HasText("▸") || tt.HasText("▾") {
		t.Fatal("the category still uses a text glyph")
	}
	page.Expanded[core.UserCache] = true
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("expanding did not animate the chevron")
	}
	tt.Advance(2 * time.Second)
	if tt.FramePending() {
		t.Fatal("the chevron never settles")
	}
}
