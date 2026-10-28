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

func TestApplyBusyDowngradesRecommended(t *testing.T) {
	item := func(path string) ScanItem {
		return ScanItem{Path: path, Recommended: true, Category: UserTemp, Label: SameText("x"), Size: 1}
	}
	cats := []CategorySummary{{Category: UserTemp, TotalSize: 2, Items: []ScanItem{item("/a"), item("/b")}}}
	busy := map[string]Busy{"/a": {App: "A"}}
	if n := ApplyBusy(cats, busy); n != 1 {
		t.Fatalf("affected = %d", n)
	}
	got := cats[0].Items[0]
	if got.Busy == nil || got.Recommended || !got.Busy.RecommendedBeforeBusy || got.Busy.App != "A" {
		t.Fatalf("downgrade = %+v busy=%+v", got, got.Busy)
	}
	if ApplyBusy(cats, busy) != 1 || !cats[0].Items[0].Busy.RecommendedBeforeBusy || cats[0].Items[0].Recommended {
		t.Fatal("second apply lost the earlier recommendation")
	}
	if cats[0].Items[1].Busy != nil || !cats[0].Items[1].Recommended {
		t.Fatal("clear item changed")
	}
	unknown := []CategorySummary{{Items: []ScanItem{item("/c")}}}
	if ApplyBusy(unknown, map[string]Busy{"/c": {Unknown: true}}) != 1 || unknown[0].Items[0].Recommended || unknown[0].Items[0].Busy == nil || !unknown[0].Items[0].Busy.RecommendedBeforeBusy {
		t.Fatal("unknown must downgrade and keep the earlier recommendation")
	}
	if ApplyBusy(cats, map[string]Busy{"/a": {}}) != 0 {
		t.Fatal("empty observation is not occupancy")
	}
}
