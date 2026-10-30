//go:build darwin || linux

package core

import (
	"bytes"
	"fmt"
	"math/rand"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
)

func TestMappedTagCandidatesDoNotCopyUnrelatedLongNames(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	v := VolumeFromMountPoint(diskCacheVolumeDir(t))
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}}
	for i := range 10_000 {
		entries = append(entries, TreeIndexEntry{Used: true, Name: fmt.Sprintf("%s-%d", strings.Repeat("ordinary-long-file", 3), i)})
	}
	if _, err := cache.Save(v, NewSizeTree(v, entries), 1); err != nil {
		t.Fatal(err)
	}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	loaded, err := cache.Load(v)
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > 64<<10 {
		t.Errorf("tag candidate harvesting copied unrelated names: %d allocated bytes", allocated)
	}
	if tags := cacheTagBitsSlow(loaded.Scan.Tree); slices.ContainsFunc(tags, func(word uint64) bool { return word != 0 }) {
		t.Fatal("ordinary long filename became a cache tag")
	}
}

func TestMappedLoadDoesNotMaterializeValidatedBase(t *testing.T) {
	cache, volume, _ := overrideFixture(t, 5_000, 39)
	base, _ := cache.Paths(volume)
	encoded := diskCacheRead(t, base)
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	loaded, err := cache.Load(volume)
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(loaded.Scan.Tree.base.mapped.close)
	t.Logf("mapped load: %d allocated bytes for %d input bytes", after.TotalAlloc-before.TotalAlloc, len(encoded))
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > uint64(len(encoded))/2 {
		t.Errorf("mapped load materialized %d bytes for a %d-byte validated base", allocated, len(encoded))
	}
	if _, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(volume.Mount, "d4999", "f38")); !ok {
		t.Fatal("mapped CSR lost a child")
	}
	clone := loaded.Scan.Tree.Clone()
	clone.UpsertFile(filepath.Join(volume.Mount, "d4999", "added"), 17)
	if _, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(volume.Mount, "d4999", "added")); ok {
		t.Fatal("mapping edits changed old snapshot")
	}
	if _, err := cache.PersistIncremental(volume, clone, 11); err != nil {
		t.Fatal(err)
	}
	runtime.GC()
	runtime.ReadMemStats(&before)
	reloaded, err := cache.Load(volume)
	runtime.ReadMemStats(&after)
	if err != nil || !reloaded.DeltaApplied || reloaded.FileCount != 195001 {
		t.Fatal("mapped delta failed", err)
	}
	t.Cleanup(reloaded.Scan.Tree.base.mapped.close)
	t.Logf("mapped load: %d allocated bytes for %d input bytes", after.TotalAlloc-before.TotalAlloc, len(encoded))
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > uint64(len(encoded))/2 {
		t.Errorf("mapped delta load revalidated %d bytes of unchanged base nodes", allocated)
	}
}

func TestMappedLoaderMatchesHeapValidationAndChildOrder(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	base, _ := cache.Paths(volume)
	good := diskCacheRead(t, base)
	rng := rand.New(rand.NewSource(19))
	for iteration := 0; iteration < 256; iteration++ {
		data := bytes.Clone(good)
		if iteration == 0 {
			// Reversing a valid CSR row must not change the old slot-order
			// preference for path lookup or capped discovery.
			cs, ca := int(cacheU32(data, 88)), int(cacheU32(data, 92))
			parent := 1
			start, stop := int(cacheU32(data, cs+parent*4)), int(cacheU32(data, cs+(parent+1)*4))
			if stop-start < 2 {
				t.Fatal("fixture needs sibling children")
			}
			x, y := cacheU32(data, ca+start*4), cacheU32(data, ca+(stop-1)*4)
			cachePut32(data, ca+start*4, y)
			cachePut32(data, ca+(stop-1)*4, x)
		} else {
			at := rng.Intn(len(data))
			data[at] ^= byte(1 << rng.Intn(8))
		}
		cachePut64(data, 72, cacheChecksum(data, 72))
		heap, heapErr := decodeDiskIndex(data, volume)
		diskCacheWrite(t, base, data)
		mapped, err := mapDiskIndex(base)
		if err != nil {
			t.Fatal(err)
		}
		decoded, mapErr := decodeDiskIndexWithMapping(mapped.bytes(), volume, mapped)
		if (heapErr == nil) != (mapErr == nil) {
			mapped.close()
			t.Fatalf("validation diverged at mutation %d: heap=%v mapped=%v", iteration, heapErr, mapErr)
		}
		if mapErr == nil {
			if decoded.meta != heap.meta {
				t.Error("mapped metadata changed")
			}
			for i := 0; i < heap.tree.nodeCount(); i++ {
				if decoded.tree.slot(uint32(i)) != heap.tree.slot(uint32(i)) {
					t.Errorf("mapped slot %d changed", i)
				}
				if !slices.Equal(decoded.tree.ChildIndices(uint32(i)), heap.tree.ChildIndices(uint32(i))) {
					t.Errorf("mapped child order changed at mutation %d node %d", iteration, i)
				}
			}
		}
		mapped.close()
	}
}
