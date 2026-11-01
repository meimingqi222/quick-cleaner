package app

import (
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

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
	if !r.confirming || len(r.confirmation.Apps) != 1 || !r.confirmation.Apps[0].Checked || !strings.Contains(r.confirmation.Apps[0].Label, "Chrome") {
		t.Fatalf("single confirmation must list the running owner checked: %+v", r.confirmation.Apps)
	}
}

func TestJunkCleanSkipsUncheckedRunningOwnerApps(t *testing.T) {
	r, q, chrome, plain := ownerCleanRoot(t)
	var attempted []string
	r.record = func(_ string, paths []string, _, _, _ uint, _ uint64) { attempted = paths }
	confirmOwnerClean(t, r, q)
	r.toggleConfirmApp(r.confirmation.ID, 0)
	r.resolveConfirmation(r.confirmation.ID, true)
	dispatchNext(t, q)
	if slices.Contains(attempted, chrome) || !slices.Contains(attempted, plain) {
		t.Fatalf("unchecked owner's cache was attempted: %v", attempted)
	}
	if !strings.Contains(r.dashboard.StatusText, "Chrome still running; 1 of its cache items skipped") {
		t.Fatalf("status = %q", r.dashboard.StatusText)
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
	dispatchNext(t, q)
	if !slices.Equal(closed, []string{"Chrome"}) || !slices.Contains(attempted, chrome) {
		t.Fatalf("closed=%v attempted=%v", closed, attempted)
	}
	if !strings.Contains(r.dashboard.StatusText, "Chrome did not quit within 8 s") {
		t.Fatalf("straggler not reported: %q", r.dashboard.StatusText)
	}
}

func TestJunkCleanWithoutRunningOwnersShowsNoAppList(t *testing.T) {
	r, q, _, _ := ownerCleanRoot(t)
	r.findRunningApps = func([]core.ScanItem) []core.RunningApp { return nil }
	r.requestJunkClean()
	dispatchNext(t, q)
	if !r.confirming || len(r.confirmation.Apps) != 0 {
		t.Fatalf("idle owners still listed: %+v", r.confirmation.Apps)
	}
}
