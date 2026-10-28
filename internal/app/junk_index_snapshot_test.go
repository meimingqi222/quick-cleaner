package app

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestJunkRefreshPublishesAndUsesNewSnapshot(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "data")
	volume := core.VolumeFromMountPoint(root)
	makeTree := func(size uint64) *core.SizeTree {
		return core.SizeTreeFromSnapshot(volume, []core.TreeSnapshotEntry{{Path: path, Size: size}})
	}
	old, fresh := makeTree(1), makeTree(9)
	t.Run("scan", func(t *testing.T) {
		trees := prioritizeJunkIndex([]*core.SizeTree{old}, fresh)
		plan := core.BeginJunkIndex(context.Background(), false, root, []core.VolumeID{volume}, trees, nil, nil)
		got, ok := plan.Query.MeasurePath(context.Background(), path)
		if !ok || got.Size != 9 {
			t.Fatalf("scan used stale snapshot: %+v", got)
		}
	})
	t.Run("search", func(t *testing.T) {
		r, _ := testRoot(t)
		r.searchTrees[volume.Mount] = old
		r.rememberJunkIndex(fresh)
		if r.searchTrees[volume.Mount] != fresh {
			t.Fatal("search kept stale snapshot")
		}
	})
}
