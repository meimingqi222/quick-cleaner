//go:build darwin || linux

package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sync/atomic"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type incrementalSpillSource struct {
	*macIndexFixture
	spill  *platform.MacWalkSpill
	root   string
	cancel context.CancelFunc
	full   atomic.Int64
}

func (s *incrementalSpillSource) Scan(ctx context.Context, v VolumeID, few bool) (ScanResult, error) {
	if !few {
		s.full.Add(1)
		return ScanResult{}, errors.New("unexpected whole-volume rebuild")
	}
	if v.Mount != s.root {
		return ScanResult{}, errors.New("unexpected subtree")
	}
	if s.cancel != nil {
		s.cancel()
	}
	return ScanResult{Volume: v, Spill: s.spill, SpillRoot: "subtree", SpillBlocked: []string{filepath.Join(s.root, "cloud")}}, nil
}

func TestMacDiskIndexSpilledSubtreeDoesNotRebuildVolume(t *testing.T) {
	for _, mode := range []string{"valid", "truncated", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			root := diskCacheVolumeDir(t)
			changed := filepath.Join(root, "changed")
			if err := os.Mkdir(changed, 0700); err != nil {
				t.Fatal(err)
			}
			v := VolumeFromMountPoint(root)
			tree := SizeTreeFromSnapshot(v, []TreeSnapshotEntry{{Path: changed, IsDir: true}, {Path: filepath.Join(changed, "old"), Size: 11}, {Path: filepath.Join(root, "keep"), Size: 101}})
			tree.SetBlockedSubtrees([]string{filepath.Join(changed, "old-gap"), filepath.Join(root, "unrelated")})
			cache, err := NewDiskIndexCache(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			if _, err := cache.Save(v, tree, 10); err != nil {
				t.Fatal(err)
			}
			original, err := cache.Load(v)
			if err != nil {
				t.Fatal(err)
			}
			entries := []TreeIndexEntry{{Name: "nested", IsDir: true, Used: true}, {Parent: 1, Name: "a", Size: 7, Used: true}, {Parent: 1, Name: "a", Size: 9, Used: true}, {Name: "sibling", Size: 5, Used: true}}
			spill := writeTestSpill(t, entries)
			defer spill.Remove()
			path := spill.Path()
			if mode == "truncated" {
				info, err := os.Stat(path)
				if err != nil {
					t.Fatal(err)
				}
				if err := os.Truncate(path, info.Size()-1); err != nil {
					t.Fatal(err)
				}
			}
			source := &incrementalSpillSource{macIndexFixture: &macIndexFixture{current: 11, changes: &platform.FSEventChanges{Paths: []string{changed}, LastEventID: 11}}, spill: spill, root: changed}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if mode == "cancelled" {
				source.cancel = cancel
			}
			owner := newMacDiskIndex(context.Background(), cache, source)
			defer owner.Close()
			updated, loadErr := owner.Load(ctx, v)
			if mode == "valid" {
				if loadErr != nil || source.full.Load() != 0 {
					t.Fatalf("spilled subtree triggered a whole-volume rebuild: %v full=%d", loadErr, source.full.Load())
				}
				if updated.TotalSize != 122 || updated.FileCount != 4 {
					t.Fatalf("spilled subtree totals changed: bytes=%d files=%d", updated.TotalSize, updated.FileCount)
				}
				if _, ok := updated.Tree.FindNodeByPath(filepath.Join(changed, "old")); ok {
					t.Fatal("old subtree survived replacement")
				}
				idx := mustNode(t, updated.Tree, filepath.Join(changed, "nested", "a"))
				if updated.Tree.SizeOf(idx) != 7 {
					t.Fatal("duplicate-name slot preference changed")
				}
				gaps := updated.Tree.BlockedSubtrees()
				if len(gaps) != 2 || !slices.Contains(gaps, filepath.Join(changed, "cloud")) || !slices.Contains(gaps, filepath.Join(root, "unrelated")) {
					t.Fatal("new or unrelated coverage lost, or old gap retained", gaps)
				}
			} else if loadErr == nil {
				t.Fatal("invalid or cancelled subtree was published")
			}
			if mode == "cancelled" && !errors.Is(loadErr, context.Canceled) {
				t.Fatal("cancelled spill changed error", loadErr)
			}
			flushMacIndex(t, owner)
			saved, err := cache.Load(v)
			if err != nil {
				t.Fatal(err)
			}
			wantEvent := uint64(10)
			if mode == "valid" {
				wantEvent = 11
			}
			if saved.LastEventID != wantEvent {
				t.Fatal("incorrect durable checkpoint", saved.LastEventID)
			}
			if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
				t.Error("consumed or abandoned subtree spill was not removed", err)
			}
			if original.Scan.Tree.SizeOf(0) != 112 || original.Scan.Tree.FileCountOf(0) != 2 {
				t.Fatal("old snapshot changed")
			}
			if _, ok := original.Scan.Tree.FindNodeByPath(filepath.Join(changed, "old")); !ok {
				t.Fatal("replacement mutated old snapshot")
			}
		})
	}
}

func TestSpillGraftMatchesMemorySubtreeAcrossLayouts(t *testing.T) {
	for _, mapped := range []bool{false, true} {
		name := "heap"
		if mapped {
			name = "mapped"
		}
		t.Run(name, func(t *testing.T) {
			v := VolumeFromMountPoint(diskCacheVolumeDir(t))
			original := NewSizeTree(v, []TreeIndexEntry{{Name: "root", Used: true, IsDir: true}, {Name: "keep", Used: true, Size: 111}})
			if mapped {
				cache, err := NewDiskIndexCache(t.TempDir())
				if err != nil {
					t.Fatal(err)
				}
				if _, err := cache.Save(v, original, 1); err != nil {
					t.Fatal(err)
				}
				loaded, err := cache.Load(v)
				if err != nil {
					t.Fatal(err)
				}
				original = loaded.Scan.Tree
			}
			sourceVolume := VolumeFromMountPoint("/source")
			entries := spillEntries(1000, 64)
			source := NewSizeTree(sourceVolume, entries)
			source.SetBlockedSubtrees([]string{"/source/dir/cloud"})
			expected := original.Clone()
			expected.AppendSubtree(0, source, "grafted")
			spill := writeTestSpill(t, entries[1:])
			defer spill.Remove()
			actual := original.Clone()
			if err := actual.appendSpillSubtree(context.Background(), 0, "grafted", &ScanResult{Volume: sourceVolume, Spill: spill, SpillBlocked: source.BlockedSubtrees()}); err != nil {
				t.Fatal(err)
			}
			if actual.nodeCount() != expected.nodeCount() {
				t.Fatal("graft count changed")
			}
			for i := 0; i < actual.nodeCount(); i++ {
				idx := uint32(i)
				if actual.slot(idx) != expected.slot(idx) || !slices.Equal(actual.ChildIndices(idx), expected.ChildIndices(idx)) {
					t.Fatal("stream and memory graft diverged", i)
				}
			}
			if !slices.Equal(actual.BlockedSubtrees(), expected.BlockedSubtrees()) {
				t.Fatal("graft coverage changed")
			}
			if original.nodeCount() != 2 || original.SizeOf(0) != 111 {
				t.Fatal("graft changed original")
			}
		})
	}
}

func TestSpillGraftRejectsMalformedRecordsAndCount(t *testing.T) {
	for _, mode := range []string{"forward-parent", "file-parent", "unsafe-name", "invalid-utf8", "too-many", "too-few", "oversized-count"} {
		t.Run(mode, func(t *testing.T) {
			entries := []TreeIndexEntry{{Used: true, IsDir: true, Name: "dir"}, {Used: true, Parent: 1, Name: "file", Size: 9}}
			switch mode {
			case "forward-parent":
				entries[0].Parent = 1
			case "file-parent":
				entries[0].IsDir = false
			case "unsafe-name":
				entries[1].Name = "../file"
			case "invalid-utf8":
				entries[1].Name = string([]byte{255})
			}
			spill := writeTestSpill(t, entries)
			defer spill.Remove()
			switch mode {
			case "too-many":
				spill = platform.OpenMacWalkSpill(spill.Path(), 1, spill.NameBytes())
			case "too-few":
				spill = platform.OpenMacWalkSpill(spill.Path(), 3, spill.NameBytes())
			case "oversized-count":
				spill = platform.OpenMacWalkSpill(spill.Path(), 1_000_000, spill.NameBytes())
			}
			tree := NewSizeTree(VolumeFromMountPoint("/fixture"), nil)
			var before, after runtime.MemStats
			runtime.ReadMemStats(&before)
			if err := tree.appendSpillSubtree(context.Background(), 0, "sub", &ScanResult{Volume: VolumeFromMountPoint("/source"), Spill: spill}); err == nil {
				t.Fatal("malformed subtree accepted")
			}
			runtime.ReadMemStats(&after)
			if mode == "oversized-count" && after.TotalAlloc-before.TotalAlloc > 1<<20 {
				t.Fatalf("untrusted count reserved %d bytes before checking spill length", after.TotalAlloc-before.TotalAlloc)
			}
		})
	}
}

func BenchmarkSpillSubtreeGraft(b *testing.B) {
	entries := spillEntries(100_000, 64)
	source := NewSizeTree(VolumeFromMountPoint("/source"), entries)
	spill := writeTestSpill(b, entries[1:])
	defer spill.Remove()
	v := VolumeFromMountPoint("/fixture")
	cache, err := NewDiskIndexCache(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	if _, err := cache.Save(v, NewSizeTree(v, nil), 1); err != nil {
		b.Fatal(err)
	}
	loaded, err := cache.Load(v)
	if err != nil {
		b.Fatal(err)
	}
	for _, streamed := range []bool{false, true} {
		name := "memory-subtree"
		if streamed {
			name = "streamed-spill"
		}
		b.Run(name, func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				tree := loaded.Scan.Tree.Clone()
				if streamed {
					if err := tree.appendSpillSubtree(context.Background(), 0, "sub", &ScanResult{Volume: source.Volume(), Spill: spill}); err != nil {
						b.Fatal(err)
					}
				} else {
					tree.AppendSubtree(0, source, "sub")
				}
				if tree.FileCountOf(0) != 100_000 || tree.SizeOf(0) != 300_000 {
					b.Fatal("graft totals changed")
				}
			}
		})
	}
}

func TestNativeScanOwnsSpillUntilSuccessfulTransfer(t *testing.T) {
	for _, mode := range []string{"success", "walk-error", "conversion-cancel"} {
		t.Run(mode, func(t *testing.T) {
			spill := writeTestSpill(t, spillEntries(2, 1)[1:])
			defer spill.Remove()
			path := spill.Path()
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var walkErr error
			if mode == "walk-error" {
				walkErr = errors.New("walk failed after spill finished")
			}
			if mode == "conversion-cancel" {
				cancel()
			}
			raw := platform.MacWalkResult{Volume: VolumeFromMountPoint("/fixture"), Spill: spill, Entries: []platform.MacWalkEntry{{Name: "fixture", Used: true, IsDir: true}}}
			scan, err := finishMacWalkScan(ctx, raw, walkErr, time.Now())
			if mode == "success" {
				if err != nil || scan.Spill != spill {
					t.Fatal("successful spill was not transferred", err)
				}
				if _, err := os.Stat(path); err != nil {
					t.Fatal("source removed transferred spill", err)
				}
			} else {
				if err == nil {
					t.Fatal("failed scan accepted")
				}
				if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
					t.Error("failed native scan leaked finished spill", err)
				}
			}
		})
	}
}
