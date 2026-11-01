package app

import (
	"os"
	"path/filepath"
	"testing"
	"time"

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
	r, q := testRoot(t)
	r.tree = core.SizeTreeFromSnapshot(vol, []core.TreeSnapshotEntry{
		{Path: dir, IsDir: true},
		{Path: sub, IsDir: true},
		{Path: gone, Size: 5},
		{Path: live, Size: 2},
	})
	r.disk.Volume = vol
	r.disk.Loaded = true
	r.disk.Space = &view.DiskPageSpace{Total: 100, Free: 50}
	r.volumeSpace = func(core.VolumeID) (uint64, uint64, bool) { return 100, 55, true }
	subIdx, ok := r.tree.FindNodeByPath(sub)
	if !ok {
		t.Fatal("fixture lacks sub")
	}
	r.currentNode = subIdx

	r.pruneDiskPaths([]string{gone, live})
	// The per-path stat runs on a worker; the index changes only when its
	// result is posted back.
	if _, found := r.tree.FindNodeByPath(gone); !found {
		t.Fatal("index pruned on the calling thread before the worker reported")
	}
	dispatchNext(t, q)
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
	dispatchNext(t, q)
	if r.tree.Valid(subIdx) {
		t.Fatal("deleted directory stayed valid")
	}
	if r.currentNode != r.tree.Root() {
		t.Fatal("position did not fall back to the surviving ancestor")
	}
}

// Pruning edits the tree that the file-search goroutine walks. Cancelling the
// search only signals it; the prune must wait until that reader has exited,
// and must not let a new search start before the edit is applied. The wait
// happens on the worker, never on the UI thread.
func TestDiskPruneWaitsForSearchReaderAndHoldsNewSearches(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	gone := filepath.Join(dir, "gone.txt")
	vol := core.VolumeFromMountPoint(dir)
	r.tree = core.SizeTreeFromSnapshot(vol, []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: gone, Size: 5}})
	r.disk.Volume = vol
	r.disk.Loaded = true
	r.volumeSpace = func(core.VolumeID) (uint64, uint64, bool) { return 0, 0, false }
	reader := make(chan struct{})
	cancelled := false
	r.searchCancel = func() { cancelled = true }
	r.searchDone = reader

	r.pruneDiskPaths([]string{gone})
	if !cancelled {
		t.Fatal("in-flight search was not cancelled before pruning")
	}
	r.setSearchQuery("gone")
	if r.searchDone != nil {
		t.Fatal("a new search started while the prune was pending")
	}
	select {
	case fn := <-q:
		fn()
		t.Fatal("prune was applied while the search reader was still running")
	case <-time.After(100 * time.Millisecond):
	}
	if _, found := r.tree.FindNodeByPath(gone); !found {
		t.Fatal("tree changed while the search reader was still running")
	}
	close(reader)
	dispatchNext(t, q)
	if _, found := r.tree.FindNodeByPath(gone); found {
		t.Fatal("prune never applied after the reader exited")
	}
	if r.searchDone == nil || !r.search.Searching {
		t.Fatal("held search query was not restarted after the prune")
	}
}

// A file moved to Trash leaves its path but still occupies the volume, so the
// free space cannot be inferred from pruned sizes. It is re-read from the
// system; when that fails, the old value stays.
func TestDiskPruneTakesFreeSpaceFromSystemNotFromPrunedSizes(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	moved := filepath.Join(dir, "moved-to-trash.txt")
	vol := core.VolumeFromMountPoint(dir)
	r.disk.Volume = vol
	r.disk.Loaded = true
	r.disk.Volumes = []view.DiskPageVolume{{ID: vol, Space: &view.DiskPageSpace{Total: 1000, Free: 200}}}
	for _, tc := range []struct {
		ok   bool
		want uint64
	}{{true, 230}, {false, 200}} {
		r.tree = core.SizeTreeFromSnapshot(vol, []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: moved, Size: 100}})
		r.disk.Space = &view.DiskPageSpace{Total: 1000, Free: 200}
		ok := tc.ok
		r.volumeSpace = func(core.VolumeID) (uint64, uint64, bool) { return 1000, 230, ok }
		r.pruneDiskPaths([]string{moved})
		dispatchNext(t, q)
		if _, found := r.tree.FindNodeByPath(moved); found {
			t.Fatal("moved path stayed in the index")
		}
		if r.disk.Space.Free != tc.want {
			t.Fatalf("system space ok=%v: free = %d, want %d (never 200+100)", tc.ok, r.disk.Space.Free, tc.want)
		}
	}
	if r.disk.Volumes[0].Space.Free != 230 {
		t.Fatalf("volume row free = %d, want the refreshed 230", r.disk.Volumes[0].Space.Free)
	}
}

// Typing again cancels search A and starts search B, but cancelling does not
// stop A at once. Waiting only for the newest search let the prune edit the
// tree while A was still walking it: every earlier reader must be done too.
func TestDiskPruneWaitsForEveryEarlierSearchReader(t *testing.T) {
	r, q := testRoot(t)
	dir := t.TempDir()
	gone := filepath.Join(dir, "gone.txt")
	vol := core.VolumeFromMountPoint(dir)
	r.tree = core.SizeTreeFromSnapshot(vol, []core.TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: gone, Size: 5}})
	r.disk.Volume = vol
	r.disk.Loaded = true
	r.volumeSpace = func(core.VolumeID) (uint64, uint64, bool) { return 0, 0, false }
	readerA := make(chan struct{})
	r.searchDone = readerA
	r.searchCancel = func() {}
	r.setSearchQuery("query B") // cancels A, starts B; B leaves at once once cancelled

	r.pruneDiskPaths([]string{gone})
	select {
	case fn := <-q:
		fn()
		t.Fatal("prune was applied while the earlier search A was still running")
	case <-time.After(200 * time.Millisecond):
	}
	if _, found := r.tree.FindNodeByPath(gone); !found {
		t.Fatal("tree changed while the earlier search A was still running")
	}
	close(readerA)
	dispatchNext(t, q)
	if _, found := r.tree.FindNodeByPath(gone); found {
		t.Fatal("prune never applied after every reader exited")
	}
}
