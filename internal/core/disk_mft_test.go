package core

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestMFTDomainAdapter(t *testing.T) {
	root := t.TempDir()
	raw := platform.MFTScanResult{Volume: VolumeFromMountPoint(root), RootRecord: 5, Entries: make([]platform.MFTEntry, 24), RecordsExpected: 24, MFTRunBytes: 4096, ExtRecords: 2}
	raw.Entries[0] = platform.MFTEntry{Parent: 5, Name: "$MFT", Used: true, Size: 40}
	raw.Entries[5] = platform.MFTEntry{Parent: 5, Name: ".", Used: true, IsDir: true}
	raw.Entries[6] = platform.MFTEntry{Parent: 5, Name: "first", Used: true, IsDir: true}
	raw.Entries[7] = platform.MFTEntry{Parent: 5, Name: "second", Used: true, IsDir: true}
	raw.Entries[8] = platform.MFTEntry{Parent: 6, Name: "large.dat", Used: true, Size: 100, Mtime: 1234, LinkParents: []uint64{6, 7}}
	raw.Entries[9] = platform.MFTEntry{BaseRecord: 8, Size: 23, LinkParents: []uint64{7, 6}}
	raw.Entries[10] = platform.MFTEntry{BaseRecord: 8, LinkParents: []uint64{7}}
	raw.Entries[11] = platform.MFTEntry{Parent: 7, Name: "small.dat", Used: true, Size: 7}
	raw.Entries[12] = platform.MFTEntry{Parent: 7, Name: "unused.dat", Size: 999}
	raw.Entries[16] = platform.MFTEntry{Parent: 5, Name: "$user", Used: true, IsDir: true}
	raw.Entries[17] = platform.MFTEntry{Parent: 16, Name: "personal.txt", Used: true, Size: 3}
	raw.Entries[18] = platform.MFTEntry{BaseRecord: 7, Size: 500}
	raw.Entries[19] = platform.MFTEntry{Parent: 999, Name: "orphan.dat", Used: true, Size: 5}
	got, err := BuildMFTScanResult(context.Background(), raw, 2)
	if err != nil {
		t.Fatal(err)
	}
	if got.UniqueSize != 178 || got.TotalSize != 301 || got.UniqueFiles != 5 || got.FileCount != 6 || got.DirCount != 4 || got.HardLinks != 1 || got.ExtDataMerged != 1 {
		t.Fatalf("extension/hard-link accounting: %+v", got)
	}
	if got.RecordsRead != 24 || got.RecordsExpected != 24 || got.MFTRunBytes != 4096 || got.ExtRecords != 2 {
		t.Fatalf("acquisition diagnostics lost: %+v", got)
	}
	tree := got.Tree
	if tree.Root() != 0 || tree.PathOf(0) != root || tree.PathOf(5) != filepath.Join(root, "$MFT") || tree.SizeOf(0) != 296 || tree.FileCountOf(0) != 5 || tree.SizeOf(6) != 123 || tree.SizeOf(7) != 130 {
		t.Fatalf("root remapping and ancestor aggregation: %+v", tree.Children(0))
	}
	children := tree.Children(0)
	if len(children) != 3 || children[0].Name != "second" || children[1].Name != "first" || children[2].Name != "$user" {
		t.Fatalf("metadata is hidden, ordinary $ names remain visible: %v", children)
	}
	if len(tree.Children(7)) != 1 || tree.Children(7)[0].Name != "small.dat" {
		t.Fatal("must not invent an alias filename from an extra hard-link parent")
	}
	if len(got.Dirs) != 2 || got.Dirs[0].Size != 296 || got.Dirs[1].Path != filepath.Join(root, "second") || got.Dirs[1].Size != 130 {
		t.Fatalf("directory ranking: %v", got.Dirs)
	}
	for _, tree := range []*SizeTree{tree, tree.Clone()} {
		if len(tree.Search("$MFT", 10)) != 0 || len(tree.Children(0)) != 3 || tree.LargestFiles(1)[0].Size != 123 {
			t.Fatal("native metadata presentation policy lost")
		}
		for _, hit := range tree.Search("", 30) {
			if hit.Name == "$MFT" || hit.Name == "." {
				t.Fatalf("metadata appears in empty search: %+v", hit)
			}
		}
	}
	if raw.Entries[8].Size != 100 || len(raw.Entries[9].LinkParents) != 2 {
		t.Fatal("adapter mutated acquisition input")
	}
	// A regular directory-walk tree must not inherit NTFS-only presentation.
	plain := NewSizeTree(VolumeFromMountPoint(root), []TreeIndexEntry{{IsDir: true, Used: true}, {Parent: 0, Name: "$MFT", Size: 2, Used: true}})
	if len(plain.Children(0)) != 1 || len(plain.Search("$MFT", 1)) != 1 {
		t.Fatal("metadata filtering leaked into non-NTFS trees")
	}
}

func TestMFTDomainRefusesInvalidRootAndCancellation(t *testing.T) {
	raw := platform.MFTScanResult{RootRecord: 5, Entries: make([]platform.MFTEntry, 6)}
	if _, err := BuildMFTScanResult(context.Background(), raw, 0); !errors.Is(err, platform.ErrMFTCorrupt) {
		t.Fatal(err)
	}
	raw.Entries[5] = platform.MFTEntry{Used: true, IsDir: true}
	raw.RootRecord = 0
	if _, err := BuildMFTScanResult(context.Background(), raw, 0); !errors.Is(err, platform.ErrMFTCorrupt) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := BuildMFTScanResult(ctx, raw, 0); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
