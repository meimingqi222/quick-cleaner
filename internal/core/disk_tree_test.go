package core

import (
	"context"
	"fmt"
	"math"
	"path/filepath"
	"testing"
)

func fixtureSizeTree(root string) *SizeTree {
	return NewSizeTree(VolumeFromMountPoint(root), []TreeIndexEntry{
		{0, root, true, 0, true, 0}, {0, "a.txt", false, 100, true, 1}, {0, "proj", true, 0, true, 0},
		{2, "b.txt", false, 200, true, 2}, {2, "src", true, 0, true, 0}, {4, "c.txt", false, 300, true, 3},
		{4, "d.txt", false, 400, true, 4}, {0, "other", true, 0, true, 0}, {7, "e.txt", false, 500, true, 5},
	})
}
func TestHardLinkParentSurvivesRecompute(t *testing.T) {
	tree := fixtureSizeTree(t.TempDir())
	before := tree.SizeOf(2)
	tree.NoteExtraParent(1, 2)
	tree.RecomputeAggregates()
	if tree.SizeOf(2) != before+100 || tree.FileCountOf(2) != 4 {
		t.Fatalf("extra parent lost: size=%d files=%d before=%d", tree.SizeOf(2), tree.FileCountOf(2), before)
	}
	again := tree.Clone()
	again.RecomputeAggregates()
	if again.SizeOf(2) != tree.SizeOf(2) {
		t.Fatal("clone dropped extra hard-link parent")
	}
}

func TestSizeTreeDomainAndIncrementalPort(t *testing.T) {
	root := t.TempDir()
	tree := fixtureSizeTree(root)
	if tree.SizeOf(0) != 1500 || tree.FileCountOf(0) != 5 || tree.CountUsedDirs() != 4 || tree.CountUsedFiles() != 5 {
		t.Fatal(tree.Children(0))
	}
	children := tree.Children(0)
	if children[0].Name != "proj" || children[0].OwnSize != 200 || children[0].Size != 900 {
		t.Fatal(children)
	}
	file := filepath.Join(root, "proj", "b.txt")
	if !tree.UpsertFile(file, 250) {
		t.Fatal("upsert")
	}
	idx, ok := tree.FindNodeByPath(file)
	if !ok || idx != 3 || tree.SizeOf(0) != 1550 || len(tree.CompactEntries()) != 9 {
		t.Fatal(idx, tree.SizeOf(0))
	}
	tree.RemoveSubtreeInplace(2)
	sub := NewSizeTree(VolumeFromMountPoint(filepath.Join(root, "proj")), []TreeIndexEntry{{0, "proj", true, 0, true, 0}, {0, "f.txt", false, 999, true, 9}})
	tree.AppendSubtree(0, sub, "proj")
	tree.RebuildChildArrays()
	if tree.SizeOf(0) != 1599 || tree.FileCountOf(0) != 3 {
		t.Fatal(tree.SizeOf(0), tree.FileCountOf(0))
	}
	tree.RecomputeAggregates()
	if tree.SizeOf(0) != 1599 || tree.FileCountOf(0) != 3 {
		t.Fatal("recompute mismatch")
	}
	if _, ok := tree.FindNodeByPath(filepath.Join(root, "proj", "f.txt")); !ok {
		t.Fatal("replacement missing")
	}
	tree.RemoveSubtreeInplace(999)
	if tree.SizeOf(0) != 1599 {
		t.Fatal("invalid removal changed totals")
	}
	restored := NewSizeTree(tree.Volume(), tree.CompactEntries())
	if restored.SizeOf(0) != 1599 || restored.CountUsedFiles() != 3 {
		t.Fatal("compact roundtrip")
	}
	tree.SetDirTotals(0, 10, 2)
	if tree.SizeOf(0) != 10 {
		t.Fatal("root totals double counted")
	}
}

func TestSizeTreeSearchOrderingCancellationAndDepth(t *testing.T) {
	root := t.TempDir()
	tree := fixtureSizeTree(root)
	if hits := tree.Search("B.TXT", 16); len(hits) != 1 || hits[0].Name != "b.txt" {
		t.Fatal(hits)
	}
	if hits := tree.Search("*.txt", 2); len(hits) != 2 || hits[0].Name != "b.txt" || hits[1].Name != "a.txt" {
		t.Fatal(hits)
	}
	if hits := tree.Search("", 2); len(hits) != 2 || hits[0].Size != 1500 || hits[1].Size != 900 {
		t.Fatal(hits)
	}
	if got := tree.LargestFiles(2); len(got) != 2 || got[0].Name != "e.txt" || got[1].Name != "d.txt" {
		t.Fatal(got)
	}
	if len(tree.Search("*", 0)) != 0 || len(tree.LargestFiles(0)) != 0 {
		t.Fatal("zero limit")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if len(tree.SearchCancellable(ctx, "", 10)) != 0 || len(tree.SearchCancellable(ctx, "*", 10)) != 0 {
		t.Fatal("cancel")
	}
	// Indexed max depth is relative to traversed directories: direct files are
	// included at depth zero, as in the Rust index method.
	if got := tree.CollectSubtreeFiles(0, 0, 0, math.MaxUint64); len(got) != 1 {
		t.Fatal(got)
	}
	if got := tree.CollectSubtreeFiles(0, 1, 0, math.MaxUint64); len(got) != 3 {
		t.Fatal(got)
	}
}

func TestSizeTreeBatchCacheReuseAndAliasRepair(t *testing.T) {
	root := t.TempDir()
	entries := []TreeIndexEntry{{0, root, true, 0, true, 0}, {0, "wide", true, 0, true, 0}}
	for i := 0; i < 256; i++ {
		entries = append(entries, TreeIndexEntry{1, fmt.Sprint("file", i), false, 0, true, 0})
	}
	tree := NewSizeTree(VolumeFromMountPoint(root), entries)
	for i := 0; i < 128; i++ {
		path := filepath.Join(root, "wide", fmt.Sprint("file", i))
		if !tree.UpsertFileWithMtime(path, 10, 100) {
			t.Fatal(path)
		}
		idx, ok := tree.FindNodeByPath(path)
		if !ok || idx != uint32(i+2) || len(tree.entries) != 258 || len(tree.childNames) != 2 {
			t.Fatal("metadata update invalidated stable slots/cache")
		}
	}
	for i := 128; i < 256; i++ {
		path := filepath.Join(root, "wide", fmt.Sprint("file", i))
		idx, _ := tree.FindNodeByPath(path)
		tree.RemoveSubtreeInplace(idx)
		if len(tree.childNames) != 2 {
			t.Fatal("removal discarded directory cache")
		}
		if _, ok := tree.FindNodeByPath(path); ok {
			t.Fatal("ghost")
		}
		if !tree.UpsertFileWithMtime(path, 20, 200) || len(tree.childNames) != 2 {
			t.Fatal("append discarded directory cache")
		}
	}
	if tree.SizeOf(0) != 3840 || tree.FileCountOf(0) != 256 {
		t.Fatal(tree.SizeOf(0), tree.FileCountOf(0))
	}
	if !tree.UpsertFile(filepath.Join(root, "wide", "FILE0"), 3) {
		t.Fatal("case rename")
	}
	idx, _ := tree.FindNodeByPath(filepath.Join(root, "wide", "file0"))
	if tree.PathOf(idx) != filepath.Join(root, "wide", "FILE0") || tree.SizeOf(0) != 3833 {
		t.Fatal(tree.PathOf(idx), tree.SizeOf(0))
	}
	alias := NewSizeTree(VolumeFromMountPoint(root), []TreeIndexEntry{{0, root, true, 0, true, 0}, {0, "Notes", true, 0, true, 0}, {1, "old", false, 4, true, 0}, {0, "notes", false, 7, true, 0}})
	if idx, _ := alias.FindNodeByPath(filepath.Join(root, "NOTES")); idx != 1 {
		t.Fatal(idx)
	}
	alias.RemoveSubtreeInplace(1)
	if idx, _ := alias.FindNodeByPath(filepath.Join(root, "NOTES")); idx != 3 {
		t.Fatal("alias repair", idx)
	}
	if _, ok := alias.FindNodeByPath(filepath.Join(root, "Notes", "old")); ok {
		t.Fatal("stale descendant")
	}
}

func TestSizeTreeBrokenPathsAndCoverage(t *testing.T) {
	root := t.TempDir()
	tree := NewSizeTree(VolumeFromMountPoint(root), []TreeIndexEntry{{0, root, true, 0, true, 0}, {1, "orphan", true, 0, true, 0}, {99, "broken", true, 0, true, 0}})
	if tree.PathOf(1) != "" || tree.PathOf(2) != "" {
		t.Fatal("invented paths")
	}
	entries := []TreeIndexEntry{{0, root, true, 0, true, 0}}
	for i := 1; i <= 600; i++ {
		entries = append(entries, TreeIndexEntry{uint32(i - 1), fmt.Sprint("d", i), true, 0, true, 0})
	}
	deep := NewSizeTree(VolumeFromMountPoint(root), entries)
	if deep.PathOf(600) != "" || deep.PathOf(10) == "" {
		t.Fatal("depth guard")
	}
	tree = fixtureSizeTree(root)
	blocked := filepath.Join(root, "proj", "cloud")
	tree.SetBlockedSubtrees([]string{blocked})
	if tree.CoversSubtree(root) || tree.CoversSubtree(filepath.Join(root, "proj")) || tree.CoversSubtree(blocked) || tree.CoversSubtree(filepath.Join(root, "missing")) || !tree.CoversSubtree(filepath.Join(root, "other")) {
		t.Fatal("coverage")
	}
	if _, ok := tree.FindNodeByPath(filepath.Join(filepath.Dir(root), "elsewhere", "a.txt")); ok {
		t.Fatal("cross-mount hit")
	}
}

func TestSizeTreeCloneSnapshotAndScanResult(t *testing.T) {
	root := t.TempDir()
	volume := VolumeFromMountPointWithLabel(root, "disk")
	tree := SizeTreeFromSnapshot(volume, []TreeSnapshotEntry{{filepath.Join(root, "dir", "a"), false, 10, 99}, {root, true, 0, 0}, {filepath.Join(root, "dir"), true, 0, 0}, {filepath.Join(root, "missing", "b"), false, 999, 0}})
	if tree.SizeOf(0) != 10 || tree.FileCountOf(0) != 1 || tree.NameOf(0) != "disk" {
		t.Fatal(tree.Children(0))
	}
	clone := tree.Clone()
	if !clone.UpsertFile(filepath.Join(root, "dir", "a"), 20) || tree.SizeOf(0) != 10 {
		t.Fatal("clone aliases original")
	}
	result := ScanResult{Volume: volume, Tree: clone, TotalSize: 20, FileCount: 1}
	result.RemovePath(filepath.Join(root, "dir"))
	if result.TotalSize != 0 || result.FileCount != 0 || clone.SizeOf(0) != 0 || tree.SizeOf(0) != 10 {
		t.Fatal(result)
	}
	subRoot := filepath.Join(root, "sub")
	sub := NewSizeTree(VolumeFromMountPoint(subRoot), nil)
	sub.SetBlockedSubtrees([]string{filepath.Join(subRoot, "cloud")})
	tree.AppendSubtree(0, sub, "replacement")
	if tree.CoversSubtree(filepath.Join(root, "replacement")) || tree.CoversSubtree(root) || len(tree.BlockedSubtrees()) != 1 {
		t.Fatal("lost appended coverage metadata")
	}
}

func TestSizeTreeAppendAndCompactRetainSearchSlotOrder(t *testing.T) {
	root := t.TempDir()
	tree := NewSizeTree(VolumeFromMountPoint(root), nil)
	sub := NewSizeTree(VolumeFromMountPoint(filepath.Join(root, "sub")), []TreeIndexEntry{{0, "sub", true, 0, true, 0}, {3, "first.txt", false, 1, true, 0}, {0, "second.txt", false, 99, true, 0}, {0, "dir", true, 0, true, 0}})
	tree.AppendSubtree(0, sub, "sub")
	if hits := tree.Search("*.txt", 1); len(hits) != 1 || hits[0].Name != "first.txt" || hits[0].Path != filepath.Join(root, "sub", "dir", "first.txt") {
		t.Fatal(hits)
	}
	restored := NewSizeTree(tree.Volume(), tree.CompactEntries())
	if hits := restored.Search("*.txt", 1); len(hits) != 1 || hits[0].Name != "first.txt" {
		t.Fatal(hits)
	}
}
