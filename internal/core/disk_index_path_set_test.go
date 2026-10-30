//go:build darwin || linux

package core

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestMacIndexRemovedPathsClearCoverageInOneBatch(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := VolumeFromMountPoint(root)
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}}
	var blocked, removed []string
	for i := range 128 {
		blocked = append(blocked, filepath.Join(root, fmt.Sprintf("blocked-%03d", i), "unknown"))
	}
	for i := range 512 {
		name := fmt.Sprintf("removed-%03d", i)
		entries = append(entries, TreeIndexEntry{Used: true, Name: name, Size: 1})
		removed = append(removed, filepath.Join(root, name))
	}
	removed = append(removed, filepath.Dir(blocked[0]))
	tree := NewSizeTree(volume, entries)
	tree.SetBlockedSubtrees(blocked)
	original := ScanResult{Volume: volume, Tree: tree}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	updated, err := refreshMacDiskIndex(context.Background(), original, &platform.FSEventChanges{Paths: removed}, &macIndexFixture{})
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	if bytes := after.TotalAlloc - before.TotalAlloc; bytes > 1<<20 {
		t.Errorf("coverage clearing allocated %d bytes across every removed-path/gap pair", bytes)
	}
	if len(updated.Tree.blocked) != 127 || len(original.Tree.blocked) != 128 {
		t.Fatal("coverage clear changed unrelated gap or old snapshot")
	}
	if updated.Tree.FileCountOf(0) != 0 || original.Tree.FileCountOf(0) != 512 {
		t.Fatal("removal totals changed")
	}
}

type blockedSubtreeSource struct{ *macIndexFixture }

func (s blockedSubtreeSource) Scan(_ context.Context, v VolumeID, _ bool) (ScanResult, error) {
	tree := NewSizeTree(v, []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}, {Used: true, IsDir: true, Name: "cloud"}})
	tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "cloud")})
	return ScanResult{Volume: v, Tree: tree}, nil
}

func TestMacIndexBatchClearPreservesNewSubtreeGaps(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(root, "changed")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	v := VolumeFromMountPoint(root)
	tree := NewSizeTree(v, []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}, {Used: true, IsDir: true, Name: "changed"}})
	tree.SetBlockedSubtrees([]string{dir})
	updated, err := refreshMacDiskIndex(context.Background(), ScanResult{Volume: v, Tree: tree}, &platform.FSEventChanges{Paths: []string{dir}}, blockedSubtreeSource{&macIndexFixture{}})
	if err != nil {
		t.Fatal(err)
	}
	if gaps := updated.Tree.BlockedSubtrees(); len(gaps) != 1 || gaps[0] != filepath.Join(dir, "cloud") {
		t.Fatalf("fresh unknown subtree lost: %v", gaps)
	}
}

func TestMacIndexPathSetMatchesLexicalCoverage(t *testing.T) {
	roots := []string{"/a/b", "/a/b-x", "/other"}
	set := make(macIndexPathSet)
	for _, root := range roots {
		set[root] = struct{}{}
	}
	for _, path := range []string{"/a/b", "/a/b/child", "/a/b/", "/a/b//child", "/a/b/../c", "/a/b-x/child", "/a/b-extra", "/other/x", "/other/../else", "/a", "/"} {
		want := false
		for _, root := range roots {
			want = want || pathWithin(path, root)
		}
		if got := set.contains(path); got != want {
			t.Errorf("coverage(%q)=%v, want %v", path, got, want)
		}
	}
}
