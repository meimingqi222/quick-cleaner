package app

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func TestJunkCleanKeepsCachesWhenOwnerCannotBeStopped(t *testing.T) {
	if runtime.GOOS != "windows" && runtime.GOOS != "darwin" {
		t.Skip("real cleanup requires Windows or macOS")
	}
	r, q := testRoot(t)
	r.settings.Language = core.English
	rules, err := core.CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	var targets []core.ScanTarget
	var paths []string
	for _, name := range []string{"owned", "other"} {
		path := filepath.Join(t.TempDir(), name)
		if err := os.Mkdir(path, 0700); err != nil {
			t.Fatal(err)
		}
		file := filepath.Join(path, "cache")
		if err := os.WriteFile(file, []byte("data"), 0600); err != nil {
			t.Fatal(err)
		}
		paths = append(paths, file)
		targets = append(targets, core.ProviderTarget(rules, "engine", "cache_candidate", path, core.SameText(name), core.UserCache, core.Operation{Kind: core.OpContents}, true))
	}
	r.junk.Scanned = true
	r.junk.Categories = core.ScanFixed(context.Background(), targets, nil)
	app := core.RunningApp{Name: core.SameText("Chrome"), Processes: []string{"chrome.exe"}, Paths: []string{targets[0].Path}}
	r.findRunningApps = func([]core.ScanItem) []core.RunningApp { return []core.RunningApp{app} }
	r.closeApps = func([]core.RunningApp, func() bool) []core.Text {
		for _, path := range paths {
			if _, err := os.Stat(path); err != nil {
				t.Error("deletion started before closing owners", err)
			}
		}
		return []core.Text{app.Name}
	}
	r.junk.Selected = map[string]bool{targets[0].Path: true, targets[1].Path: true}
	r.requestJunkClean()
	dispatchNext(t, q)
	r.resolveConfirmation(r.confirmation.ID, true)
	dispatchNext(t, q)
	if !strings.Contains(r.dashboard.StatusText, "their associated selected cleanup items were skipped") {
		t.Fatalf("missing immediate close failure: %s", r.dashboard.StatusText)
	}
	for r.job != nil {
		dispatchNext(t, q)
	}
	if _, err := os.Stat(paths[0]); err != nil {
		t.Fatalf("still-running app cache changed: %v", err)
	}
	if _, err := os.Stat(paths[1]); !os.IsNotExist(err) {
		t.Fatalf("other cache not cleaned: %v", err)
	}
}

func ownerCleanRoot(t *testing.T) (*Root, chan func(), string, string) {
	t.Helper()
	r, q := testRoot(t)
	r.settings.Language = core.English
	dir := t.TempDir()
	chrome, plain := filepath.Join(dir, "chrome-cache"), filepath.Join(dir, "plain-cache")
	owner := &core.AppOwner{Name: core.SameText("Chrome"), Processes: []string{"chrome.exe"}}
	r.junk.Scanned = true
	r.junk.Categories = []core.CategorySummary{{Category: core.BrowserCache, Items: []core.ScanItem{
		{Path: chrome, Size: 7, Recommended: true, Owner: owner},
		{Path: plain, Size: 5, Recommended: true},
	}}}
	r.junk.Selected = map[string]bool{chrome: true, plain: true}
	r.findRunningApps = func(items []core.ScanItem) []core.RunningApp {
		if len(items) != 2 {
			t.Errorf("checked owners for %d items", len(items))
		}
		return []core.RunningApp{{Name: owner.Name, Processes: owner.Processes, Paths: []string{chrome}, Bytes: 7}}
	}
	return r, q, chrome, plain
}

func confirmOwnerClean(t *testing.T, r *Root, q chan func()) {
	t.Helper()
	r.requestJunkClean()
	if !r.junk.Preparing || r.confirming {
		t.Fatal("owner check must run off the UI thread before the confirmation")
	}
	dispatchNext(t, q)
	if !r.confirming || r.confirmation.Kind != view.ConfirmCloseApps || len(r.confirmation.Apps) != 1 || !r.confirmation.Apps[0].Checked || r.confirmation.Apps[0].Name != "Chrome" || r.confirmation.Apps[0].Size != "7 B" {
		t.Fatalf("close-apps confirmation must list the running owner checked: kind=%v apps=%+v", r.confirmation.Kind, r.confirmation.Apps)
	}
}

func TestJunkCleanSkipsUncheckedRunningOwnerApps(t *testing.T) {
	r, q, chrome, plain := ownerCleanRoot(t)
	var attempted []string
	r.record = func(_ string, paths []string, _, _, _ uint, _ uint64) { attempted = paths }
	confirmOwnerClean(t, r, q)
	r.toggleConfirmApp(r.confirmation.ID, 0)
	r.resolveConfirmation(r.confirmation.ID, true)
	for r.job != nil {
		dispatchNext(t, q)
	}
	if slices.Contains(attempted, chrome) || !slices.Contains(attempted, plain) {
		t.Fatalf("unchecked owner's cache was attempted: %v", attempted)
	}
	if r.junk.Selected[chrome] {
		t.Fatal("skipped running app cache must be deselected in the list")
	}
	if !strings.Contains(r.dashboard.StatusText, "Chrome still running; 1 associated selected cleanup items skipped") {
		t.Fatalf("status = %q", r.dashboard.StatusText)
	}
}

func TestJunkCleanSkipAndCleanButtonLeavesBusyCachesUnselected(t *testing.T) {
	r, q, chrome, plain := ownerCleanRoot(t)
	var attempted []string
	r.record = func(_ string, paths []string, _, _, _ uint, _ uint64) { attempted = paths }
	confirmOwnerClean(t, r, q)
	r.toggleAllConfirmApps(r.confirmation.ID)
	if r.confirmation.Apps[0].Checked {
		t.Fatal("toggleAllConfirmApps did not uncheck all")
	}
	r.toggleAllConfirmApps(r.confirmation.ID)
	if !r.confirmation.Apps[0].Checked {
		t.Fatal("toggleAllConfirmApps did not re-check all")
	}
	r.skipConfirmation(r.confirmation.ID)
	for r.job != nil {
		dispatchNext(t, q)
	}
	if slices.Contains(attempted, chrome) || !slices.Contains(attempted, plain) {
		t.Fatalf("skipConfirmation attempted running owner's cache: %v", attempted)
	}
	if r.junk.Selected[chrome] {
		t.Fatal("skipped running app cache remained selected")
	}
	if r.junk.CleanBanner == nil || !r.junk.CleanBanner.SkippedActive {
		t.Fatalf("expected CleanBanner with SkippedActive, got %+v", r.junk.CleanBanner)
	}
}

func TestJunkCleanQuitsCheckedOwnerAppsFirst(t *testing.T) {
	r, q, chrome, _ := ownerCleanRoot(t)
	var attempted []string
	r.record = func(_ string, paths []string, _, _, _ uint, _ uint64) { attempted = paths }
	var closed []string
	r.closeApps = func(apps []core.RunningApp, _ func() bool) []core.Text {
		for _, app := range apps {
			closed = append(closed, app.Name.Get(core.English))
		}
		return []core.Text{core.SameText("Chrome")}
	}
	confirmOwnerClean(t, r, q)
	r.resolveConfirmation(r.confirmation.ID, true)
	for r.job != nil {
		dispatchNext(t, q)
	}
	if !slices.Equal(closed, []string{"Chrome"}) || !slices.Contains(attempted, chrome) {
		t.Fatalf("closed=%v attempted=%v", closed, attempted)
	}
	if r.junk.Selected[chrome] {
		t.Fatal("straggler owner cache remained selected")
	}
	if !strings.Contains(r.dashboard.StatusText, "Chrome could not be confirmed stopped") {
		t.Fatalf("straggler not reported: %q", r.dashboard.StatusText)
	}
}

func TestJunkCleanWithoutRunningOwnersCleansWithoutDialog(t *testing.T) {
	r, q, chrome, plain := ownerCleanRoot(t)
	var attempted []string
	r.record = func(_ string, paths []string, _, _, _ uint, _ uint64) { attempted = paths }
	r.findRunningApps = func([]core.ScanItem) []core.RunningApp { return nil }
	r.requestJunkClean()
	dispatchNext(t, q)
	if r.confirming || r.confirmation.Open {
		t.Fatalf("idle cleanup opened a confirmation dialog: %+v", r.confirmation)
	}
	if !r.junk.Cleaning {
		t.Fatal("idle cleanup did not start immediately after owner check")
	}
	for r.job != nil {
		dispatchNext(t, q)
	}
	if !slices.Contains(attempted, chrome) || !slices.Contains(attempted, plain) {
		t.Fatalf("idle cleanup did not attempt all selected items: %v", attempted)
	}
}
