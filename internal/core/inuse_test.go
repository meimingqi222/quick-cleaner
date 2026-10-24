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
