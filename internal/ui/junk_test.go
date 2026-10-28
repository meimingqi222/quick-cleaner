package ui_test

import (
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func TestJunkBlockedBadgeHidesBusy(t *testing.T) {
	blocked := core.NewRuleRef(nil, "fixture", "")
	page := appui.JunkPage{
		Language: appui.English, Scanned: true, Expanded: map[core.CategoryID]bool{core.UserCache: true},
		Categories: []core.CategorySummary{{Category: core.UserCache, Items: []core.ScanItem{{
			Path: "/fixture/blocked", Label: core.NewText("拦截", "Blocked"), Rule: blocked, Busy: &core.Busy{App: "Example"},
		}}}},
	}
	tt := ui.NewTester(func(c *ui.Context) { page.RenderContent(c, appui.JunkPageActions{}) }, 1280, 800)
	if !tt.HasText("Conflicting rule constraints; cleanup blocked") || tt.HasText("Example") {
		t.Fatalf("blocked row: %v", tt.Texts())
	}
}

func TestJunkPageSelectionAndIndependentActions(t *testing.T) {
	page := appui.JunkPage{
		Language: appui.English, Scanned: true,
		Categories: []core.CategorySummary{
			{Category: core.UserCache, TotalSize: 19, Items: []core.ScanItem{
				{Path: "/fixture/cache", Label: core.NewText("示例缓存", "Example cache"), Size: 7, FileCount: 1234, Recommended: true, Busy: &core.Busy{App: "Example"}},
				{Path: "/fixture/cache-two", Label: core.NewText("另一缓存", "Second cache"), Size: 12},
			}},
			{Category: core.DevBuild, TotalSize: 23, Partial: true},
		},
		Selected:        map[string]bool{"/fixture/cache": true, "/stale-selection": true},
		Expanded:        map[core.CategoryID]bool{core.UserCache: true},
		ReleaseEligible: map[string]bool{"/fixture/cache": true},
	}
	var toggled, excluded, released string
	var batch []appui.JunkSelection
	requests := 0
	actions := appui.JunkPageActions{
		ToggleItem:   func(path string) { toggled = path },
		Exclude:      func(path string) { excluded = path },
		ReleaseCache: func(path string) { released = path },
		Select:       func(selection appui.JunkSelection) { batch = append(batch, selection) },
		RequestClean: func() { requests++ },
	}
	tt := ui.NewTester(func(c *ui.Context) {
		appui.Dashboard{Language: page.Language}.RenderPage(c, appui.Junk, appui.DashboardActions{}, func() { page.RenderContent(c, actions) })
	}, 1280, 880)
	for _, text := range []string{"Example running", "1,234 files", "Manual Select", "Partial", "≥ 23 B", "7 B", "(1 items)", "2 items"} {
		if !tt.HasText(text) {
			t.Fatalf("missing result/selection fact %q; texts: %v", text, tt.Texts())
		}
	}
	if err := tt.Click("Quit apps and clean"); err != nil {
		t.Fatal(err)
	}
	if released != "/fixture/cache" || toggled != "" || requests != 0 {
		t.Fatal("release must request its own confirmation, not toggle or clean the selection")
	}
	// Both item rows have Exclude: use the first visible control, as a real pointer does.
	if err := tt.Click("Exclude"); err != nil {
		t.Fatal(err)
	}
	if excluded != "/fixture/cache" || toggled != "" {
		t.Fatalf("exclusion also toggled selection: exclude=%q toggle=%q", excluded, toggled)
	}
	if err := tt.Click("Example cache"); err != nil {
		t.Fatal(err)
	}
	if toggled != "/fixture/cache" {
		t.Fatalf("selected path = %q", toggled)
	}
	for _, label := range []string{"Recommended", "Select All", "Invert", "Clear All"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if len(batch) != 4 || batch[0] != appui.JunkRecommended || batch[1] != appui.JunkAll || batch[2] != appui.JunkInvert || batch[3] != appui.JunkNone {
		t.Fatalf("batch choices = %v", batch)
	}
	if err := tt.Click("Clean Now"); err != nil {
		t.Fatal(err)
	}
	if requests != 1 {
		t.Fatal("clean must request confirmation")
	}
	for _, state := range []string{"scanning", "cleaning", "not-scanned", "no-current-selection"} {
		page.Scanned, page.Scanning, page.Cleaning = true, false, false
		page.Selected["/fixture/cache"] = true
		switch state {
		case "scanning":
			page.Scanning = true
		case "cleaning":
			page.Cleaning = true
		case "not-scanned":
			page.Scanned = false
		case "no-current-selection":
			page.Selected["/fixture/cache"] = false
		}
		tt.Frame()
		label := "Clean Now"
		if page.Cleaning {
			label = "Cleaning…"
		}
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
		if requests != 1 {
			t.Fatalf("%s authorized cleanup", state)
		}
	}
}

// The page body owns only the space left under the top bar. Claiming the whole
// column height pushes the docked capsule past the content area and over the
// global status bar once the window stops being tall enough to hide it.
func TestJunkCleanBarStaysAboveTheStatusBar(t *testing.T) {
	const status = "Loaded 194 apps, about 227.67 GB in total"
	dash := appui.Dashboard{Language: appui.English, StatusText: status}
	for _, height := range []int{900, 800, 700, 600} {
		items := make([]core.ScanItem, 146)
		for i := range items {
			items[i] = core.ScanItem{Path: "/fixture/cache/" + strconv.Itoa(i), Label: core.NewText("示例缓存", "Example cache"), Size: 7}
		}
		// A ListState belongs to one window, so each height needs its own page.
		page := appui.JunkPage{
			Language: appui.English, Scanned: true,
			Categories: []core.CategorySummary{
				{Category: core.UserCache, TotalSize: 1022, Items: items},
				{Category: core.DevBuild, TotalSize: 200, Items: []core.ScanItem{
					{Path: "/fixture/build", Label: core.NewText("构建产物", "Build output"), Size: 200},
				}},
			},
			Selected: map[string]bool{"/fixture/cache/0": true},
			Expanded: map[core.CategoryID]bool{core.UserCache: true},
		}
		tt := ui.NewTester(func(c *ui.Context) {
			dash.RenderPage(c, appui.Junk, appui.DashboardActions{}, func() {
				page.RenderContent(c, appui.JunkPageActions{})
			})
		}, 1456, height)
		clean, ok := tt.Find("Clean Now")
		statusBar, okStatus := tt.Find(status)
		if !ok || !okStatus {
			t.Fatalf("%d: clean button found=%v, status bar found=%v; texts: %v", height, ok, okStatus, tt.Texts())
		}
		if bottom := clean.Y + clean.H; bottom > statusBar.Y {
			t.Fatalf("%d: clean bar bottom %.1f overlaps the status bar at %.1f", height, bottom, statusBar.Y)
		}
	}
}

func TestJunkPageExclusionsFailuresAndLocalization(t *testing.T) {
	page := appui.JunkPage{Language: appui.English, Scanned: true, Exclusions: []string{"/fixture/protected"}, FailedDetails: []string{"In use · /fixture/busy"}}
	removed := ""
	actions := appui.JunkPageActions{
		ToggleExclusions:    func() { page.ShowExclusions = !page.ShowExclusions },
		RemoveExclusion:     func(path string) { removed = path },
		ToggleFailedDetails: func() { page.ShowFailedDetails = !page.ShowFailedDetails },
	}
	tt := ui.NewTester(func(c *ui.Context) { page.RenderContent(c, actions) }, 1090, 810)
	if tt.HasText("/fixture/protected") || tt.HasText("In use · /fixture/busy") {
		t.Fatal("closed detail panels leaked their content")
	}
	if err := tt.Click("Exclusions (1)"); err != nil {
		t.Fatal(err)
	}
	if !tt.HasText("These paths are never touched by any cleanup") {
		t.Fatal("exclusion protection explanation missing")
	}
	if err := tt.Click("Remove"); err != nil {
		t.Fatal(err)
	}
	if removed != "/fixture/protected" {
		t.Fatalf("remove exclusion = %q", removed)
	}
	if err := tt.Click("Show details ▾"); err != nil {
		t.Fatal(err)
	}
	if !tt.HasText("In use · /fixture/busy") {
		t.Fatal("failure not shown on expansion")
	}
	page.Language = appui.Chinese
	tt.Frame()
	if !tt.HasText("排除清单 (1)") || !tt.HasText("收起详情 ▴") || !tt.HasText("上次清理有 1 个目标仍有残留") {
		t.Fatal("expanded panels did not translate")
	}
	if err := tt.Click("收起详情 ▴"); err != nil {
		t.Fatal(err)
	}
	if tt.HasText("In use · /fixture/busy") {
		t.Fatal("closed failure detail remained visible")
	}
}

// A scan that found nothing must say so, not list zero-byte categories with
// selection buttons that act on nothing.
func TestJunkEmptyResultIsACleanState(t *testing.T) {
	page := appui.JunkPage{
		Language: appui.English, Scanned: true, RecommendedSelected: true,
		Categories: []core.CategorySummary{{Category: core.UserCache}, {Category: core.BrowserCache}},
		Expanded:   map[core.CategoryID]bool{core.UserCache: true},
	}
	selected := 0
	tt := ui.NewTester(func(c *ui.Context) {
		page.RenderContent(c, appui.JunkPageActions{Select: func(appui.JunkSelection) { selected++ }})
	}, 1280, 800)
	if !tt.HasText("Your system is clean") || tt.HasText("Nothing cleanable found in this category") {
		t.Fatalf("empty result: %v", tt.Texts())
	}
	for _, label := range []string{"Recommended", "Select All", "Invert", "Clear All"} {
		_ = tt.Click(label)
	}
	if selected != 0 {
		t.Fatalf("%d selection buttons acted on an empty result", selected)
	}
	page.Discovering = true
	tt.Frame()
	if tt.HasText("Your system is clean") {
		t.Fatal("the clean state appeared before discovery finished")
	}
}

func TestJunkPageUnscannedStateRendersStartScanPrompt(t *testing.T) {
	page := appui.JunkPage{
		Language: appui.Chinese,
		Scanned:  false,
		Scanning: false,
	}
	scanned := 0
	actions := appui.JunkPageActions{
		Rescan: func() { scanned++ },
	}
	tt := ui.NewTester(func(c *ui.Context) {
		page.RenderContent(c, actions)
	}, 1280, 800)
	if !tt.HasText("尚未扫描系统冗余垃圾") {
		t.Fatalf("unscanned page did not show prompt: %v", tt.Texts())
	}
	if err := tt.Click("开始全面扫描"); err != nil {
		t.Fatalf("clicking start scan button failed: %v", err)
	}
	if scanned != 1 {
		t.Fatalf("expected 1 rescan call, got %d", scanned)
	}
}
