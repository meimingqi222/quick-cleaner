package core

import (
	"encoding/binary"
	"os"
	"path/filepath"
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

func writeTestSpill(t *testing.T, entries []TreeIndexEntry) *platform.MacWalkSpill {
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

func mustNode(t *testing.T, tree *SizeTree, path string) uint32 {
	t.Helper()
	idx, ok := tree.FindNodeByPath(path)
	if !ok {
		t.Fatal("missing", path)
	}
	return idx
}
