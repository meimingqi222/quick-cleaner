package core

import (
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"
)

func TestCleanContentsKeepsRootAndAccountsAllocatedBytes(t *testing.T) {
	root := t.TempDir()
	var expected uint64
	for _, name := range []string{"first", "nested/second"} {
		path := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(strings.Repeat(name, 731)), 0600); err != nil {
			t.Fatal(err)
		}
		md, err := os.Stat(path)
		if err != nil {
			t.Fatal(err)
		}
		if runtime.GOOS == "windows" {
			expected += uint64(md.Size())
		} else {
			stat := reflect.Indirect(reflect.ValueOf(md.Sys()))
			expected += uint64(stat.FieldByName("Blocks").Int()) * 512
		}
	}
	p := NewCleanProgress(2, expected)
	r := CleanDirContents(root, p)
	entries, err := os.ReadDir(root)
	if err != nil || len(entries) != 0 || len(r.Failed) != 0 || r.OK != 2 {
		t.Fatalf("root contents: %v, %v; report=%+v", entries, err, r)
	}
	s := p.Snapshot()
	if s.Files != 2 || s.Bytes != expected || s.Failed != 0 || s.Ratio() != 1 {
		t.Fatalf("accounting=%+v expected bytes=%d", s, expected)
	}
}

func TestCleanerRecursiveWhitelistAndCancellation(t *testing.T) {
	old := CurrentWhitelist()
	defer ReloadWhitelist(old)
	root := t.TempDir()
	keep := filepath.Join(root, "nested", "keep")
	if err := os.MkdirAll(filepath.Dir(keep), 0700); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, keep, "protected")
	identityWrite(t, filepath.Join(root, "remove"), "remove")
	ReloadWhitelist([]string{keep})
	p := &CleanProgress{}
	if result := CleanPath(root, p); result != CleanFailed {
		t.Fatalf("nonempty protected subtree result=%v", result)
	}
	if _, err := os.Stat(keep); err != nil {
		t.Fatalf("whitelist lost: %v", err)
	}
	if _, err := os.Stat(filepath.Join(root, "remove")); !os.IsNotExist(err) {
		t.Fatalf("unprotected sibling was not removed: %v", err)
	}
	p.RequestCancel()
	if got := CleanPath(keep, p); got != CleanSkipped {
		t.Fatalf("cancelled result=%v", got)
	}
	if _, err := os.Stat(keep); err != nil {
		t.Fatal(err)
	}
}

func TestCleanerNeverTraversesLinks(t *testing.T) {
	outside := t.TempDir()
	sentinel := filepath.Join(outside, "sentinel")
	identityWrite(t, sentinel, "must survive")
	root := t.TempDir()
	link := filepath.Join(root, "link")
	if err := os.Symlink(outside, link); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	r := CleanDirContents(root, &CleanProgress{})
	if len(r.Failed) > 0 {
		t.Fatalf("unlink failed: %+v", r)
	}
	if _, err := os.Lstat(link); !os.IsNotExist(err) {
		t.Fatalf("link remains: %v", err)
	}
	if content, err := os.ReadFile(sentinel); err != nil || string(content) != "must survive" {
		t.Fatalf("link target changed: %q %v", content, err)
	}
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	if r := CleanDirContents(link, &CleanProgress{}); len(r.Failed) != 1 {
		t.Fatalf("linked contents root accepted: %+v", r)
	}
	if _, err := os.Stat(sentinel); err != nil {
		t.Fatal(err)
	}
}

func TestCleanerRetainsNestedLiveSQLiteFamily(t *testing.T) {
	root := t.TempDir()
	db := filepath.Join(root, "nested", "Cache.db")
	if err := os.Mkdir(filepath.Dir(db), 0700); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, db, "main")
	identityWrite(t, db+"-wal", "journal")
	identityWrite(t, filepath.Join(root, "ordinary"), "disposable")
	p := &CleanProgress{}
	if got := CleanPath(root, p); got != CleanFailed {
		t.Fatalf("live nested family result=%v", got)
	}
	for _, path := range []string{db, db + "-wal"} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("live member lost: %s %v", path, err)
		}
	}
	if _, err := os.Stat(filepath.Join(root, "ordinary")); !os.IsNotExist(err) {
		t.Fatalf("ordinary sibling remains: %v", err)
	}
}

func TestRecycleFailureNeverDeletesAndSuccessNeverFreesBytes(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "keep")
	identityWrite(t, path, "original")
	p := &CleanProgress{}
	if got := recyclePathWith(path, p, func(string) error { return errors.New("denied") }); got != CleanFailed {
		t.Fatalf("error result=%v", got)
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatal("trash failure permanently deleted file")
	}
	if got := recyclePathWith(path, p, func(string) error { return nil }); got != CleanFailed {
		t.Fatalf("false success accepted: %v", got)
	}
	if got := recyclePathWith(path, p, func(path string) error { return os.Rename(path, path+".trashed") }); got != CleanOK {
		t.Fatalf("real move failed: %v", got)
	}
	if s := p.Snapshot(); s.Bytes != 0 || s.Files != 1 {
		t.Fatalf("recycled bytes counted as released: %+v", s)
	}
}

func TestFrozenArbitrarySelectionRejectsReplacement(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "target")
	identityWrite(t, path, "old")
	target := CaptureArbitraryTarget(path)
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, path, "new content")
	r := CleanArbitraryItems([]ArbitraryTarget{target}, Permanent, &CleanProgress{})
	if r.OK != 0 || len(r.Failed) != 1 {
		t.Fatalf("replacement report=%+v", r)
	}
	if content, err := os.ReadFile(path); err != nil || string(content) != "new content" {
		t.Fatalf("replacement lost: %q %v", content, err)
	}
}

func TestCleanerStaleDatabaseRequiresKnownVacancy(t *testing.T) {
	root := t.TempDir()
	db := filepath.Join(root, "Cache.db")
	identityWrite(t, db, "main")
	identityWrite(t, db+"-wal", "journal")
	old := time.Now().Add(-2 * time.Hour)
	for _, path := range []string{db, db + "-wal", root} {
		if err := os.Chtimes(path, old, old); err != nil {
			t.Fatal(err)
		}
	}
	open, known := IsOpen(root)
	p := &CleanProgress{}
	got := CleanPath(root, p)
	if !known || open {
		if got != CleanFailed {
			t.Fatalf("unconfirmed vacancy authorized deletion: %v", got)
		}
		for _, path := range []string{db, db + "-wal"} {
			if _, err := os.Stat(path); err != nil {
				t.Fatalf("unconfirmed family changed: %v", err)
			}
		}
		t.Logf("safe refusal: occupancy open=%v known=%v; successful stale-family disposal requires a known-clear host", open, known)
		return
	}
	if got != CleanOK {
		t.Fatalf("stale confirmed-vacant family refused: %v; reasons=%v", got, p.reasons)
	}
	if _, err := os.Stat(root); !os.IsNotExist(err) {
		t.Fatalf("stale family remains: %v", err)
	}
}
