package core

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestSQLiteFamilyNames(t *testing.T) {
	for _, name := range []string{"Cache.DB", "Cache.DB-WAL", "CACHE.db-SHM", "Cache.db-journal"} {
		if key, ok := SQLiteFamilyKey(name); !ok || key != "cache.db" {
			t.Fatalf("%s: %q %v", name, key, ok)
		}
	}
	for _, name := range []string{"thumb.db.backup", "db", "file.sqlite.zip", "wal"} {
		if _, ok := SQLiteFamilyKey(name); ok {
			t.Fatal(name)
		}
	}
	if !IsSQLiteCompanionName("Cache.DB-WAL") || IsSQLiteCompanionName("Cache.db") {
		t.Fatal("companion classification")
	}
}
func TestDatabaseGuardEvidence(t *testing.T) {
	dir := t.TempDir()
	db := filepath.Join(dir, "Cache.db")
	wal := db + "-wal"
	write := func(p string) {
		t.Helper()
		if e := os.WriteFile(p, []byte("db"), 0600); e != nil {
			t.Fatal(e)
		}
	}
	write(db)
	if IsLiveDatabase(db) || !HoldsLiveDatabase(dir) {
		t.Fatal("standalone file versus directory policy")
	}
	write(filepath.Join(dir, "Other.db-wal"))
	if IsLiveDatabase(db) {
		t.Fatal("unrelated companion")
	}
	write(wal)
	calls := 0
	clear := func(string) (bool, bool) { calls++; return false, true }
	if !liveDatabaseWithProbe(db, clear) || calls != 0 {
		t.Fatal("fresh family must not probe")
	}
	old := time.Now().Add(-2 * time.Hour)
	for _, p := range []string{db, wal, filepath.Join(dir, "Other.db-wal"), dir} {
		if e := os.Chtimes(p, old, old); e != nil {
			t.Fatal(e)
		}
	}
	if liveDatabaseWithProbe(db, clear) || liveDatabaseWithProbe(dir, clear) {
		t.Fatal("stale clear family")
	}
	for _, probe := range []func(string) (bool, bool){func(string) (bool, bool) { return false, false }, func(string) (bool, bool) { return true, true }} {
		if !liveDatabaseWithProbe(db, probe) {
			t.Fatal("unknown or held must protect")
		}
	}
	if e := os.Chtimes(wal, time.Now(), time.Now()); e != nil {
		t.Fatal(e)
	}
	if !liveDatabaseWithProbe(db, clear) {
		t.Fatal("fresh sibling protects stale main")
	}
	if !IsLiveDatabase(filepath.Join(dir, "missing")) || !HoldsLiveDatabase(filepath.Join(dir, "missing")) {
		t.Fatal("unreadable fail closed")
	}
}
