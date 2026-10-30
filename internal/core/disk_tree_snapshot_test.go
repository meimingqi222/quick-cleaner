//go:build darwin || linux

package core

import (
	"fmt"
	"path/filepath"
	"runtime"
	"testing"
)

func TestMappedSnapshotEditsStayLocal(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	for i := range 50_000 {
		tree.UpsertFile(filepath.Join(volume.Mount, "project", fmt.Sprintf("file-%d", i)), 1)
	}
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	original := loaded.Scan.Tree
	t.Cleanup(original.base.mapped.close)
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	clone := original.Clone()
	clone.RebuildChildArrays()
	refreshDiskScanTotals(&ScanResult{Tree: clone})
	runtime.ReadMemStats(&after)
	if bytes := after.TotalAlloc - before.TotalAlloc; bytes > 256*1024 {
		t.Errorf("quiet mapped snapshot allocated %d bytes for unchanged nodes", bytes)
	}
	added := filepath.Join(volume.Mount, "project", "added")
	if !clone.UpsertFile(added, 12) {
		t.Fatal("append failed")
	}
	clone.RemoveSubtreeInplace(2)
	clone.RebuildChildArrays()
	if _, ok := clone.FindNodeByPath(added); !ok {
		t.Fatal("new child lost")
	}
	if _, ok := original.FindNodeByPath(added); ok {
		t.Fatal("append mutated previous snapshot")
	}
	if !original.Valid(2) || clone.Valid(2) {
		t.Fatal("removal crossed snapshot boundary")
	}
	next := clone.Clone()
	if !next.UpsertFile(filepath.Join(volume.Mount, "project", "next"), 13) {
		t.Fatal("second append failed")
	}
	if _, ok := clone.FindNodeByPath(filepath.Join(volume.Mount, "project", "next")); ok {
		t.Fatal("overlay append mutated previous snapshot")
	}
	clone.UpsertFile(filepath.Join(volume.Mount, "project", "sibling"), 14)
	if _, ok := next.FindNodeByPath(filepath.Join(volume.Mount, "project", "sibling")); ok {
		t.Fatal("sibling overlay shares writable edges")
	}
	if _, err := cache.PersistIncremental(volume, next, 2); err != nil {
		t.Fatal(err)
	}
	reloaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(reloaded.Scan.Tree.base.mapped.close)
	if _, ok := reloaded.Scan.Tree.FindNodeByPath(added); !ok || reloaded.Scan.Tree.Valid(2) {
		t.Fatal("delta lost append/removal")
	}
}

func TestMappedAppendGrowthDoesNotReallocateWholeTreePerFile(t *testing.T) {
	cache, volume, _ := diskCacheFixture(t)
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}, {Used: true, IsDir: true, Name: "updates"}}
	for i := range 25_000 {
		entries = append(entries, TreeIndexEntry{Used: true, Name: fmt.Sprintf("inert-%d", i), Size: 1})
	}
	if _, err := cache.Save(volume, NewSizeTree(volume, entries), 1); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(loaded.Scan.Tree.base.mapped.close)
	clone := loaded.Scan.Tree.Clone()
	if _, ok := clone.FindNodeByPath(filepath.Join(volume.Mount, "updates")); !ok {
		t.Fatal("missing fixture parent")
	}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	for i := range 128 {
		if !clone.UpsertFile(filepath.Join(volume.Mount, "updates", fmt.Sprintf("new-%d", i)), 7) {
			t.Fatal("append failed")
		}
	}
	runtime.ReadMemStats(&after)
	if bytes := after.TotalAlloc - before.TotalAlloc; bytes > 1<<20 {
		t.Errorf("128 appends allocated %d bytes by repeatedly growing full-tree scratch", bytes)
	}
	if clone.FileCountOf(0) != 25_128 || clone.SizeOf(0) != 25_000+128*7 {
		t.Fatal("append totals changed")
	}
	if loaded.Scan.FileCount != 25_000 || loaded.Scan.Tree.FileCountOf(0) != 25_000 {
		t.Fatal("append modified original snapshot")
	}
}

func TestMappedCountsMatchLiveSlotsAcrossSnapshotChanges(t *testing.T) {
	cache, v, _ := overrideFixture(t, 32, 15)
	loaded, err := cache.Load(v)
	if err != nil {
		t.Fatal(err)
	}
	original := loaded.Scan.Tree
	check := func(tree *SizeTree) {
		t.Helper()
		var files, dirs uint64
		for idx := 0; idx < tree.nodeCount(); idx++ {
			e := tree.slot(uint32(idx))
			if !e.Used {
				continue
			}
			if e.IsDir {
				dirs++
			} else {
				files++
			}
		}
		if tree.CountUsedFiles() != files || tree.CountUsedDirs() != dirs {
			t.Fatalf("live slot counts differ: got %d/%d, want %d/%d files/dirs", tree.CountUsedFiles(), tree.CountUsedDirs(), files, dirs)
		}
	}
	check(original)
	changed := original.Clone()
	changed.RemoveSubtreeInplace(mustNode(t, changed, filepath.Join(v.Mount, "d3")))
	subtree := NewSizeTree(VolumeFromMountPoint("/new"), []TreeIndexEntry{{Name: "new", Used: true, IsDir: true}, {Name: "file", Used: true, Size: 17}})
	changed.AppendSubtree(0, subtree, "new")
	changed.UpsertFile(filepath.Join(v.Mount, "d7", "added"), 9)
	changed.RemoveSubtreeInplace(mustNode(t, changed, filepath.Join(v.Mount, "d4", "f2")))
	check(changed)
	check(original)
	// Counts include every Used slot, regardless of root aggregate semantics.
	root := changed.slot(0)
	root.Size, root.files = 0, 0
	changed.setSlot(0, root)
	check(changed)
	if _, err := cache.PersistIncremental(v, changed, 12); err != nil {
		t.Fatal(err)
	}
	reloaded, err := cache.Load(v)
	if err != nil {
		t.Fatal(err)
	}
	check(reloaded.Scan.Tree)
	changed.RemoveSubtreeInplace(mustNode(t, changed, filepath.Join(v.Mount, "new")))
	check(changed)
	check(reloaded.Scan.Tree)
}

func BenchmarkMappedLiveCounts(b *testing.B) {
	v := VolumeFromMountPoint(b.TempDir())
	cache, err := NewDiskIndexCache(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	if _, err := cache.Save(v, NewSizeTree(v, spillEntries(200_000, 16)), 1); err != nil {
		b.Fatal(err)
	}
	loaded, err := cache.Load(v)
	if err != nil {
		b.Fatal(err)
	}
	tree := loaded.Scan.Tree.Clone()
	tree.UpsertFile(filepath.Join(v.Mount, "added"), 7)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if tree.CountUsedDirs() != 17 || tree.CountUsedFiles() != 200001 {
			b.Fatal("count changed")
		}
	}
}
