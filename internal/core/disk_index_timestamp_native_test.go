//go:build darwin || linux

package core

import (
	"context"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestMacIndexFutureFileTimestampPersistsAcrossRestart(t *testing.T) {
	root, err := filepath.EvalSymlinks(diskCacheVolumeDir(t))
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "future.bin")
	macIndexWrite(t, path, 4096)
	future := time.Unix(int64(math.MaxUint32)+3600, 0)
	if err := os.Chtimes(path, future, future); err != nil {
		t.Fatal(err)
	}
	volume := VolumeFromMountPoint(root)
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	tree := NewSizeTree(volume, []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}, {Used: true, Name: "future.bin", Size: 4096}})
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	source := &macIndexFixture{current: 2, changes: &platform.FSEventChanges{Paths: []string{path}, LastEventID: 2}}
	owner := newMacDiskIndex(context.Background(), cache, source)
	defer owner.Close()
	if _, err := owner.Load(context.Background(), volume); err != nil {
		t.Fatal(err)
	}
	if err := owner.Flush(context.Background()); err != nil {
		t.Fatalf("future file blocked durable checkpoint: %v", err)
	}
	loaded, err := cache.Load(volume)
	if err != nil || loaded.LastEventID != 2 {
		t.Fatal("checkpoint was not published", err)
	}
	idx, ok := loaded.Scan.Tree.FindNodeByPath(path)
	if !ok || loaded.Scan.Tree.slot(idx).Mtime != math.MaxUint32 {
		t.Fatal("future time wrapped into an old timestamp")
	}
	restarted := newMacDiskIndex(context.Background(), cache, source)
	defer restarted.Close()
	if _, err := restarted.Cached(context.Background(), volume); err != nil || restarted.eventID != 2 {
		t.Fatal("restart replayed from stale checkpoint", err)
	}
}
