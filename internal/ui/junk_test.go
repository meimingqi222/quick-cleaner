package ui_test

import (
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
