//go:build darwin || linux

package core

import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync/atomic"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestSpillBuildMatchesMemoryBuild(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	volume := VolumeID{Mount: "/fixture", Label: "Fixture"}
	entries := []TreeIndexEntry{
		{Name: "Fixture", IsDir: true, Used: true},
		{Parent: 0, Name: "docs", IsDir: true, Used: true},
		{Parent: 1, Name: "readme", Used: true, Size: 100, Mtime: 10},
		{Parent: 0, Name: "notes", Used: true, Size: 50, Mtime: 11},
		{Parent: 1, Name: "readme", Used: true, Size: 7, Mtime: 12},
	}
	memory := NewSizeTree(volume, entries)
	spill := writeTestSpill(t, entries[1:])
	defer spill.Remove()

	built, err := BuildMacWalkSpillIndex(cache, volume, "Fixture", spill, []string{"/fixture/docs"}, 42)
	if err != nil {
		t.Fatal(err)
	}
	if built.Tree.SizeOf(0) != memory.SizeOf(0) || built.FileCount != memory.CountUsedFiles() || built.DirCount != memory.CountUsedDirs() {
		t.Fatalf("spill build diverges: got size %d files %d dirs %d, memory size %d files %d dirs %d",
			built.Tree.SizeOf(0), built.FileCount, built.DirCount, memory.SizeOf(0), memory.CountUsedFiles(), memory.CountUsedDirs())
	}
	docs := mustNode(t, built.Tree, "/fixture/docs")
	if built.Tree.SizeOf(docs) != memory.SizeOf(mustNode(t, memory, "/fixture/docs")) {
		t.Fatal("docs subtree diverges", built.Tree.SizeOf(docs))
	}
	readme := mustNode(t, built.Tree, "/fixture/docs/readme")
	if got := built.Tree.SizeOf(readme); got != 100 {
		t.Fatal("first readme lost its size", got)
	}
	var second uint32
	for _, child := range built.Tree.ChildIndices(mustNode(t, built.Tree, "/fixture/docs")) {
		if child != readme && built.Tree.EntryName(child) == "readme" {
			second = child
		}
	}
	if second == 0 || built.Tree.SizeOf(second) != 7 {
		t.Fatal("second readme missing", second)
	}
	if got := built.Tree.SizeOf(mustNode(t, built.Tree, "/fixture/notes")); got != 50 {
		t.Fatal("sibling file lost", got)
	}
	if got := built.Tree.BlockedSubtrees(); len(got) != 1 || got[0] != "/fixture/docs" {
		t.Fatal("blocked coverage lost", got)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.LastEventID != 42 || loaded.TotalSize != built.Tree.SizeOf(0) || len(loaded.Scan.Tree.BlockedSubtrees()) != 1 {
		t.Fatalf("published index lost checkpoint or coverage: %+v blocked %v", loaded.DiskIndexMetadata, loaded.Scan.Tree.BlockedSubtrees())
	}
	if _, err := os.Stat(spill.Path()); err != nil {
		t.Fatal("build removed the spill, which the caller owns", err)
	}
}

func writeTestSpill(t testing.TB, entries []TreeIndexEntry) *platform.MacWalkSpill {
	t.Helper()
	path := filepath.Join(t.TempDir(), "qc-spill-test.bin")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	var nameBytes uint64
	for _, entry := range entries {
		header := make([]byte, 19)
		binary.LittleEndian.PutUint32(header[0:4], entry.Parent)
		if entry.IsDir {
			header[4] = 1
		}
		binary.LittleEndian.PutUint32(header[5:9], uint32(entry.Mtime))
		binary.LittleEndian.PutUint64(header[9:17], entry.Size)
		binary.LittleEndian.PutUint16(header[17:19], uint16(len(entry.Name)))
		if _, err = file.Write(append(header, []byte(entry.Name)...)); err != nil {
			t.Fatal(err)
		}
		nameBytes += 2 + uint64(len(entry.Name))
	}
	return platform.OpenMacWalkSpill(path, uint64(len(entries)), nameBytes)
}

func spillEntries(n, depth int) []TreeIndexEntry {
	entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "Fixture"}}
	for i := range depth {
		entries = append(entries, TreeIndexEntry{Parent: uint32(i), Used: true, IsDir: true, Name: "dir"})
	}
	for i := range n {
		entries = append(entries, TreeIndexEntry{Parent: uint32(depth), Used: true, Name: fmt.Sprintf("payload-%d", i), Size: 3, Mtime: 17})
	}
	return entries
}

type spillFixtureSource struct {
	*macIndexFixture
	spill *platform.MacWalkSpill
}

func (s spillFixtureSource) Scan(context.Context, VolumeID, bool) (ScanResult, error) {
	return ScanResult{Spill: s.spill, SpillRoot: "Fixture", SpillBlocked: []string{"/fixture/cloud"}}, nil
}

type observingSpillWriter struct {
	*DiskIndexCache
	saves atomic.Int64
}

func (w *observingSpillWriter) Save(v VolumeID, t *SizeTree, event uint64) (DiskIndexSaveResult, error) {
	w.saves.Add(1)
	return w.DiskIndexCache.Save(v, t, event)
}

func TestSpillOwnerPublishesOnceAndReturnsMappedBase(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	v := VolumeID{Mount: "/fixture", Label: "Fixture"}
	spill := writeTestSpill(t, spillEntries(100, 4)[1:])
	source := spillFixtureSource{&macIndexFixture{current: 7}, spill}
	owner := newMacDiskIndex(context.Background(), cache, source)
	defer owner.Close()
	writer := &observingSpillWriter{DiskIndexCache: cache}
	owner.writer = writer
	scan, err := owner.Load(context.Background(), v)
	if err != nil {
		t.Fatal(err)
	}
	flushMacIndex(t, owner)
	if writer.saves.Load() != 0 {
		t.Errorf("already published spill was saved again %d times", writer.saves.Load())
	}
	if scan.Tree.base == nil {
		t.Error("spill result materialized a heap tree instead of the published base")
	}
	loaded, err := cache.Load(v)
	if err != nil || loaded.LastEventID != 7 || loaded.Scan.FileCount != 100 || loaded.TotalSize != 300 {
		t.Fatal("spill checkpoint changed", err)
	}
	if gaps := loaded.Scan.Tree.BlockedSubtrees(); len(gaps) != 1 || gaps[0] != "/fixture/cloud" {
		t.Fatal("spill coverage lost", gaps)
	}
}

func TestSpillBuildDoesNotMaterializeBaseAndRejectsTrailingName(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	v := VolumeID{Mount: "/fixture", Label: "Fixture"}
	entries := spillEntries(100_000, 16)
	spill := writeTestSpill(t, entries[1:])
	defer spill.Remove()
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	built, err := BuildMacWalkSpillIndex(cache, v, "Fixture", spill, nil, 7)
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > uint64(len(entries))*64 {
		t.Errorf("spill build allocated %d bytes for %d nodes", allocated, len(entries))
	}
	if built.FileCount != 100_000 || built.TotalSize != 300_000 || built.DirCount != 17 {
		t.Fatal("deep spill totals changed")
	}
	base, _ := cache.Paths(v)
	old := diskCacheRead(t, base)
	bad := writeTestSpill(t, spillEntries(1, 0)[1:])
	defer bad.Remove()
	f, err := os.OpenFile(bad.Path(), os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	var header [19]byte
	binary.LittleEndian.PutUint16(header[17:], 1)
	if _, err = f.Write(header[:]); err != nil {
		t.Fatal(err)
	}
	if err = f.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := BuildMacWalkSpillIndex(cache, v, "Fixture", bad, nil, 8); err == nil {
		t.Error("trailing record with missing name was published")
	}
	if !bytes.Equal(old, diskCacheRead(t, base)) {
		t.Error("malformed spill replaced the previous valid checkpoint")
	}
}

func BenchmarkSpillIndexBuild(b *testing.B) {
	entries := spillEntries(100_000, 64)
	spill := writeTestSpill(b, entries[1:])
	defer spill.Remove()
	cache, err := NewDiskIndexCache(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	v := VolumeID{Mount: "/fixture", Label: "Fixture"}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := BuildMacWalkSpillIndex(cache, v, "Fixture", spill, nil, uint64(i+1)); err != nil {
			b.Fatal(err)
		}
	}
}

func TestSpillRejectsStaleOrMalformedWithoutReplacingCheckpoint(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	v := VolumeID{Mount: "/fixture", Label: "Fixture"}
	entries := spillEntries(2, 2)
	spill := writeTestSpill(t, entries[1:])
	defer spill.Remove()
	if _, err := BuildMacWalkSpillIndex(cache, v, "Fixture", spill, []string{"/fixture/cloud"}, 10); err != nil {
		t.Fatal(err)
	}
	base, _ := cache.Paths(v)
	previous := diskCacheRead(t, base)
	for _, fromDisk := range []bool{false, true} {
		if fromDisk {
			diskIndexWrites.Lock()
			delete(diskIndexWrites.watermarks, base)
			diskIndexWrites.Unlock()
		}
		if _, err := BuildMacWalkSpillIndex(cache, v, "Fixture", spill, nil, 9); !errors.Is(err, ErrDiskIndexStale) {
			t.Fatalf("stale checkpoint accepted (fromDisk=%t): %v", fromDisk, err)
		}
		if !bytes.Equal(previous, diskCacheRead(t, base)) {
			t.Fatal("stale checkpoint replaced newer base")
		}
	}
	for _, tc := range []struct {
		name string
		edit func([]byte) []byte
	}{
		{"partial-header", func(b []byte) []byte { return b[:10] }},
		{"partial-name", func(b []byte) []byte { return b[:20] }},
		{"forward-parent", func(b []byte) []byte { binary.LittleEndian.PutUint32(b, 2); return b }},
		{"unsafe-name", func(b []byte) []byte { b[19] = '/'; return b }},
		{"file-parent", func(b []byte) []byte { b[4] = 0; return b }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			bad := writeTestSpill(t, entries[1:])
			defer bad.Remove()
			data, err := os.ReadFile(bad.Path())
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(bad.Path(), tc.edit(data), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := BuildMacWalkSpillIndex(cache, v, "Fixture", bad, nil, 11); err == nil {
				t.Fatal("malformed spill accepted")
			}
			if !bytes.Equal(previous, diskCacheRead(t, base)) {
				t.Fatal("malformed spill replaced valid base")
			}
			loaded, err := cache.Load(v)
			if err != nil || loaded.LastEventID != 10 || len(loaded.Scan.Tree.BlockedSubtrees()) != 1 {
				t.Fatal("old checkpoint or coverage became unreadable", err)
			}
		})
	}
}

func mustNode(t *testing.T, tree *SizeTree, path string) uint32 {
	t.Helper()
	idx, ok := tree.FindNodeByPath(path)
	if !ok {
		t.Fatal("missing", path)
	}
	return idx
}
