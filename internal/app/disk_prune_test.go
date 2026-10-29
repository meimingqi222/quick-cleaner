package app

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// Deleting a file in the disk lens must prune the in-memory index, not
// rescan the volume and lose the position. The Rust reference does this
// through prune_deleted_from_mft.
func TestDiskDeletePrunesTreeAndKeepsPosition(t *testing.T) {
	dir := t.TempDir()
	sub := filepath.Join(dir, "sub")
	live := filepath.Join(sub, "live.txt")
	if err := os.MkdirAll(sub, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(live, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	gone := filepath.Join(sub, "gone.txt")
	vol := core.VolumeFromMountPoint(dir)
	r := NewRoot(core.Settings{}, func(fn func()) { fn() }, func() {})
	r.tree = core.SizeTreeFromSnapshot(vol, []core.TreeSnapshotEntry{
		{Path: dir, IsDir: true},
		{Path: sub, IsDir: true},
		{Path: gone, Size: 5},
		{Path: live, Size: 2},
	})
	r.disk.Volume = vol
	r.disk.Loaded = true
	r.disk.Space = &view.DiskPageSpace{Total: 100, Free: 50}
	subIdx, ok := r.tree.FindNodeByPath(sub)
	if !ok {
		t.Fatal("fixture lacks sub")
	}
	r.currentNode = subIdx

	r.pruneDiskPaths([]string{gone, live})
	if _, found := r.tree.FindNodeByPath(gone); found {
		t.Fatal("gone.txt stayed in the index")
	}
	if _, found := r.tree.FindNodeByPath(live); !found {
		t.Fatal("live.txt was pruned though it still exists on disk")
	}
	if r.currentNode != subIdx {
		t.Fatal("position moved after a partial delete")
	}
	if r.disk.Space.Free != 55 {
		t.Fatalf("free space = %d, want 55", r.disk.Space.Free)
	}

	if err := os.RemoveAll(sub); err != nil {
		t.Fatal(err)
	}
	r.pruneDiskPaths([]string{sub})
	if r.tree.Valid(subIdx) {
		t.Fatal("deleted directory stayed valid")
	}
	if r.currentNode != r.tree.Root() {
		t.Fatal("position did not fall back to the surviving ancestor")
	}
}
