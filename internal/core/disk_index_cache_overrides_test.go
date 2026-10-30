//go:build darwin || linux

package core

import (
	"bytes"
	"path/filepath"
	"runtime"
	"strconv"
	"testing"
)

// overrideFixture saves a base of dirs dirs with files files each and loads it
// back mapped, so later writes land in overrides.
func overrideFixture(t *testing.T, dirs, files int) (*DiskIndexCache, VolumeID, *SizeTree) {
	t.Helper()
	mount := diskCacheVolumeDir(t)
	volume := VolumeFromMountPoint(mount)
	entries := []TreeIndexEntry{{Name: filepath.Base(mount), IsDir: true, Used: true}}
	for d := range dirs {
		dir := uint32(len(entries))
		entries = append(entries, TreeIndexEntry{Name: "d" + strconv.Itoa(d), IsDir: true, Used: true})
		for f := range files {
			entries = append(entries, TreeIndexEntry{Parent: dir, Name: "f" + strconv.Itoa(f), Size: uint64(f + 1), Used: true, Mtime: 7})
		}
	}
	cache, err := NewDiskIndexCache(filepath.Join(t.TempDir(), "cache"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := cache.Save(volume, NewSizeTree(volume, entries), 10); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.Scan.Tree.base == nil {
		t.Fatal("fixture base is not mapped")
	}
	return cache, volume, loaded.Scan.Tree
}

func TestOverrideDeltaMatchesFullEncoderByteForByte(t *testing.T) {
	cache, volume, tree := overrideFixture(t, 20, 5)
	mount := volume.Mount
	for _, p := range []string{"d1/new-a", "d1/new-b", "d7/f2", "d9/f0"} {
		if !tree.UpsertFileWithMtime(filepath.Join(mount, p), 4096, 9) {
			t.Fatal("upsert", p)
		}
	}
	tree.RemoveSubtreeInplace(mustNode(t, tree, filepath.Join(mount, "d3")))
	sub := NewSizeTree(VolumeFromMountPoint("/sub"), []TreeIndexEntry{{Name: "sub", IsDir: true, Used: true}, {Name: "inner", Size: 5, Used: true}})
	tree.AppendSubtree(mustNode(t, tree, filepath.Join(mount, "d4")), sub, "grafted")

	result, err := cache.PersistIncremental(volume, tree, 11)
	if err != nil || !result.Delta {
		t.Fatalf("incremental persist: %+v %v", result, err)
	}
	basePath, deltaPath := cache.Paths(volume)
	fast := diskCacheRead(t, deltaPath)
	decoded, err := decodeDiskIndex(diskCacheRead(t, basePath), volume)
	if err != nil {
		t.Fatal(err)
	}
	meta := DiskIndexMetadata{LastEventID: 11, ScannedAt: cacheU64(fast, 28), FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	full, _, err := encodeDiskIndexDelta(tree, decoded, meta)
	if err != nil {
		t.Fatal(err)
	}
	// [100,128) is the checksum and coverage extension that publication adds;
	// both paths publish through the same function.
	if len(fast) != len(full) || !bytes.Equal(fast[:100], full[:100]) || !bytes.Equal(fast[128:], full[128:]) {
		t.Fatal("override-only delta differs from the full encoder")
	}
	reloaded, err := cache.Load(volume)
	if err != nil || !reloaded.DeltaApplied || reloaded.Scan.Tree.SizeOf(0) != tree.SizeOf(0) {
		t.Fatalf("delta does not reload: %+v %v", reloaded, err)
	}
}

func TestIncrementalPersistDoesNotCopyTheBase(t *testing.T) {
	cache, volume, tree := overrideFixture(t, 25_000, 7)
	for i := range 20 {
		if !tree.UpsertFile(filepath.Join(volume.Mount, "d"+strconv.Itoa(i), "new"), 4096) {
			t.Fatal("upsert")
		}
	}
	basePath, _ := cache.Paths(volume)
	size := int64(len(diskCacheRead(t, basePath)))
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	result, err := cache.PersistIncremental(volume, tree, 11)
	runtime.ReadMemStats(&after)
	if err != nil || !result.Delta {
		t.Fatalf("incremental persist: %+v %v", result, err)
	}
	// Reading, decoding or validating the whole base allocates at least its size.
	if allocated := int64(after.TotalAlloc - before.TotalAlloc); allocated >= size {
		t.Fatalf("incremental persist allocated %d bytes for a %d-byte base", allocated, size)
	}
}

func TestValidatedLargeOverlayCompactsWithoutRedecodingBase(t *testing.T) {
	cache, volume, tree := overrideFixture(t, 5_000, 39)
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "large"}}
	for i := 0; i <= DiskIndexDeltaCompactThreshold/2; i++ {
		entries = append(entries, TreeIndexEntry{Used: true, Name: "added-" + strconv.Itoa(i), Size: 3})
	}
	tree.AppendSubtree(mustNode(t, tree, filepath.Join(volume.Mount, "d0")), NewSizeTree(VolumeFromMountPoint("/large"), entries), "large")
	tree.RemoveSubtreeInplace(mustNode(t, tree, filepath.Join(volume.Mount, "d1", "f0")))
	gap := filepath.Join(volume.Mount, "d2", "unknown")
	tree.SetBlockedSubtrees([]string{gap})
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	result, err := cache.PersistIncremental(volume, tree, 11)
	runtime.ReadMemStats(&after)
	if err != nil || !result.Compacted || result.Delta {
		t.Fatalf("large valid overlay did not compact: %+v %v", result, err)
	}
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > 80<<20 {
		t.Errorf("validated compaction allocated %d bytes by decoding the old base again", allocated)
	}
	loaded, err := cache.Load(volume)
	if err != nil || loaded.LastEventID != 11 || loaded.Scan.FileCount != tree.FileCountOf(0) || loaded.TotalSize != tree.SizeOf(0) {
		t.Fatal("compacted snapshot or watermark changed", err)
	}
	if _, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(volume.Mount, "d0", "large", "added-100000")); !ok {
		t.Fatal("appended subtree lost")
	}
	if _, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(volume.Mount, "d1", "f0")); ok {
		t.Fatal("deleted file resurrected")
	}
	if loaded.Scan.Tree.CoversSubtree(filepath.Join(volume.Mount, "d2")) {
		t.Fatal("compaction lost unknown coverage")
	}
}
