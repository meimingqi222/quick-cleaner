package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestStaleDatabaseWithRealHandle(t *testing.T) {
	if runtime.GOOS == "darwin" {
		t.Skip("macOS deliberately excludes this process and descendants from lsof")
	}
	dir := t.TempDir()
	db := filepath.Join(dir, "Cache.db")
	wal := db + "-wal"
	for _, p := range []string{db, wal} {
		if err := os.WriteFile(p, []byte("db"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	old := time.Now().Add(-2 * time.Hour)
	for _, p := range []string{db, wal, dir} {
		if err := os.Chtimes(p, old, old); err != nil {
			t.Fatal(err)
		}
	}
	f, err := os.Open(db)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if !IsLiveDatabase(db) || !IsLiveDatabase(dir) {
		t.Fatal("stale but held family must remain protected")
	}
	if SpotCheckInUse([]string{db, dir})[db] != SpotBusy {
		t.Fatal("held target must fail spot check")
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
	if open, known := IsOpen(db); known && !open && IsLiveDatabase(db) {
		t.Fatal("stale confirmed vacant family should clear")
	}
}

func TestInUseRealHeldFile(t *testing.T) {
	p := filepath.Join(t.TempDir(), "held.bin")
	f, e := os.Create(p)
	if e != nil {
		t.Fatal(e)
	}
	defer f.Close()
	if runtime.GOOS != "darwin" {
		if open, known := IsOpen(p); !known || !open {
			t.Fatalf("held file: open=%v known=%v", open, known)
		}
	}
	missing := p + "missing"
	if _, known := IsOpen(missing); known {
		t.Fatal("missing raw probe is unknown")
	}
	if SpotCheckInUse([]string{missing})[missing] != SpotClear {
		t.Fatal("missing deletion target is clear")
	}
}
func TestBusyBadgePolicy(t *testing.T) {
	if !(Busy{}).IsEmpty() {
		t.Fatal("default busy")
	}
	text, app, ok := (Busy{App: "Edge", Open: true, Unknown: true}).Badge()
	if !ok || !app || text.Get(English) != "Edge running" {
		t.Fatal("app priority")
	}
	text, app, ok = (Busy{Unknown: true}).Badge()
	if !ok || app || text.Get(English) != "Busy status unknown" {
		t.Fatal("unknown badge")
	}
}

func TestSplitFailuresKeepsRealErrorsInMixedTargets(t *testing.T) {
	p := NewCleanProgress(0, 0)
	var failures []CleanFailure
	for i, reason := range []FailReason{FailInUse, FailAccessDenied, FailUnverified, FailChanged, FailOther} {
		path := filepath.Join("/fixture/cache", string(rune('a'+i)))
		p.fail(path, reason)
		failures = append(failures, CleanFailure{Path: path})
	}
	failed, skipped := p.SplitFailures(failures)
	if len(failed) != 3 || len(skipped) != 2 || failed[0] != failures[1] || skipped[1] != failures[2] {
		t.Fatalf("wrong leaf classification: failed=%v skipped=%v", failed, skipped)
	}
}

// Occupancy is a badge. Recommendation, selectability and cleanable bytes
// follow the rule; locked leaves are skipped by the cleaner, not by the list.
func TestApplyBusyKeepsRecommendationAndSelection(t *testing.T) {
	item := func(path string) ScanItem {
		return ScanItem{Path: path, Recommended: true, Category: UserTemp, Label: SameText("x"), Size: 1}
	}
	cats := []CategorySummary{{Category: UserTemp, TotalSize: 3, Items: []ScanItem{item("/a"), item("/b"), item("/c")}}}
	busy := map[string]Busy{"/a": {App: "A"}, "/c": {Unknown: true}}
	if n := ApplyBusy(cats, busy); n != 2 {
		t.Fatalf("affected = %d", n)
	}
	for _, got := range cats[0].Items {
		if !got.Recommended || !got.CleanupAvailable() {
			t.Fatalf("occupancy changed recommendation or selection: %+v busy=%+v", got, got.Busy)
		}
	}
	if cats[0].Items[0].Busy == nil || cats[0].Items[0].Busy.App != "A" || cats[0].Items[2].Busy == nil || !cats[0].Items[2].Busy.Unknown {
		t.Fatal("busy badge evidence was not attached")
	}
	if cats[0].Items[1].Busy != nil {
		t.Fatal("clear item changed")
	}
	if got := cats[0].CleanableSize(); got != 3 {
		t.Fatalf("cleanable bytes = %d; occupancy must not shrink the estimate", got)
	}
	if ApplyBusy(cats, map[string]Busy{"/b": {}}) != 0 {
		t.Fatal("empty observation is not occupancy")
	}
}

func TestApplyBusyDoesNotDisableNativeResources(t *testing.T) {
	cats := []CategorySummary{{Items: []ScanItem{{Path: "docker://fixture", Operation: Operation{Kind: OpDocker}, Recommended: true}}}}
	ApplyBusy(cats, map[string]Busy{"docker://fixture": {Unknown: true}})
	if cats[0].Items[0].Busy != nil || !cats[0].Items[0].Recommended {
		t.Fatal("filesystem occupancy disabled native operation")
	}
}
