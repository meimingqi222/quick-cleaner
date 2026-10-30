package core

import (
	"context"
	"math"
	"path/filepath"
	"testing"
)

func TestPersistenceQuarantinesUnrepresentableNodes(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	project := filepath.Join(volume.Mount, "project")
	live := loaded.Scan.Tree
	if !live.UpsertFileWithMtime(filepath.Join(project, "bad\xffname"), 10, 5) ||
		!live.UpsertFileWithMtime(filepath.Join(project, "future.bin"), 20, math.MaxUint32+1) {
		t.Fatal("fixture edits rejected")
	}
	m := newMacDiskIndex(context.Background(), cache, nil)
	defer m.Close()
	m.queuePersistence(&loaded.Scan, 2, true)
	if err := m.Flush(context.Background()); err != nil {
		t.Fatalf("one unencodable node blocked the checkpoint: %v", err)
	}
	if _, ok := live.FindNodeByPath(filepath.Join(project, "bad\xffname")); !ok {
		t.Error("quarantine mutated the published snapshot")
	}

	reloaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	if reloaded.LastEventID != 2 {
		t.Fatalf("watermark %d, want 2", reloaded.LastEventID)
	}
	saved := reloaded.Scan.Tree
	if _, ok := saved.FindNodeByPath(filepath.Join(project, "bad\xffname")); ok {
		t.Error("unencodable name was persisted")
	}
	if saved.CoversSubtree(project) {
		t.Error("parent of a dropped node still claims full coverage")
	}
	idx, ok := saved.FindNodeByPath(filepath.Join(project, "future.bin"))
	if !ok || saved.slot(idx).Mtime != math.MaxUint32 {
		t.Error("future timestamp was not saturated")
	}
	if _, ok := saved.FindNodeByPath(filepath.Join(project, "keep.bin")); !ok {
		t.Error("valid sibling was lost")
	}
}
