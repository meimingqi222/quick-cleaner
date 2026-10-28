package platform

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestFSEventReplayContinuation(t *testing.T) {
	c := fseventCollector{root: "/"}
	c.add("/Users/a", 0, 14)
	c.add("/Users/b", 0, 12)
	c.add("/System/Volumes/Data/Users/a", 0, 15)
	got, err := c.changes(10, 9999, "", nil, 0)
	if err != nil || !got.Incomplete || got.LastEventID != 15 || !reflect.DeepEqual(got.Paths, []string{"/Users/a", "/Users/b"}) {
		t.Fatalf("partial replay must not skip undelivered history: %+v, %v", got, err)
	}
	c.historyDone = true
	got, err = c.changes(10, 22, "", nil, 0)
	if err != nil || got.Incomplete || got.LastEventID != 22 {
		t.Fatalf("completed history may use native latest ID: %+v, %v", got, err)
	}
	for _, id := range []uint64{0, 9, 10} {
		c := fseventCollector{root: "/", maxID: id}
		if got, err := c.changes(10, 9999, "", nil, 0); err == nil || got != nil {
			t.Fatalf("no delivered progress (%d) must refuse the batch: %+v, %v", id, got, err)
		}
	}
}

func TestFSEventScopedRescansAndCacheFiltering(t *testing.T) {
	c := fseventCollector{root: "/"}
	c.add("/Library/Keychains", 0x01|0x04, 12)
	c.add("/System/Volumes/Data/Library/Keychains", 0x02, 13)
	c.add("/custom/cache/index.bin", 0x01, 14)
	c.add("/custom/cache", 0, 15)
	c.add("/custom/cache-neighbor/a", 0, 16)
	c.add("", 0x10, 17)
	got, err := c.changes(10, 17, "/custom/cache", nil, 0)
	if err != nil || got.RequiresFullScan || got.Incomplete || got.FilteredCacheEvents != 2 || got.RawEventCount != 5 {
		t.Fatalf("subtree overflow must not force a whole-volume scan: %+v, %v", got, err)
	}
	if !reflect.DeepEqual(got.MustRescan, []string{"/Library/Keychains"}) || !reflect.DeepEqual(got.Paths, []string{"/Library/Keychains", "/custom/cache-neighbor/a"}) {
		t.Fatalf("unexpected paths: %+v", got)
	}
	for _, tc := range []struct {
		flags  uint32
		reason string
	}{{0x08, "EventIdsWrapped"}, {0x20, "RootChanged"}, {0x28, "EventIdsWrapped"}} {
		c.add("/other", tc.flags, 18)
		got, err := c.changes(10, 18, "/custom/cache", nil, 0)
		if err != nil || !got.RequiresFullScan || got.FullScanReason != tc.reason {
			t.Fatalf("volume flag %x: %+v, %v", tc.flags, got, err)
		}
		c.events = c.events[:len(c.events)-1]
	}
}

func TestFSEventRootRescanExpansion(t *testing.T) {
	for _, path := range []string{"/", "/System/Volumes/Data"} {
		c := fseventCollector{root: "/"}
		c.add("/before", 0, 11)
		c.add(path, 0x01, 12)
		c.add("/after", 0, 13)
		got, err := c.changes(10, 999, "", []string{"/Users", "/System/Volumes/Data/Library", "/Library"}, 77)
		if err != nil || got.RequiresFullScan || got.Incomplete || got.LastEventID != 77 || len(got.Paths) != 0 || got.RawEventCount != 1 || !reflect.DeepEqual(got.MustRescan, []string{"/Library", "/Users"}) {
			t.Fatalf("root %q must expand without replay prefix: %+v, %v", path, got, err)
		}
		got, err = c.changes(10, 999, "", nil, 77)
		if err != nil || !got.RequiresFullScan || got.FullScanReason != "RootMustScanSubDirs" || got.LastEventID != 10 {
			t.Fatalf("unreadable/empty root must request full scan without advancing: %+v, %v", got, err)
		}
	}
	for _, path := range []string{"/", "/System/Volumes/DataBase", "/Library"} {
		c := fseventCollector{root: "/"}
		c.add(path, 0, 1)
		if c.rootRescan {
			t.Fatalf("ordinary event must not trigger root rescan: %s", path)
		}
	}
}

func TestFSEventCanonicalPathsAndDirectChildren(t *testing.T) {
	for input, want := range map[string]string{
		"/System/Volumes/Data": "/", "/System/Volumes/Data/Users/A": "/Users/A",
		"/System/Volumes/DataBase": "/System/Volumes/DataBase", "/System/Volumes/Preboot/x": "/System/Volumes/Preboot/x",
	} {
		if got := CanonicalFSEventPath(input); got != want {
			t.Errorf("%q → %q, want %q", input, got, want)
		}
	}
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "child"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "file"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if got := fseventDirectChildren(root); !reflect.DeepEqual(got, []string{filepath.Join(root, "child")}) {
		t.Fatalf("only direct directories: %v", got)
	}
	if got := fseventDirectChildren(filepath.Join(root, "missing")); len(got) != 0 {
		t.Fatalf("missing root: %v", got)
	}
}
