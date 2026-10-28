package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestResidualAndAppSizesUseOnlyCoveredIndexes(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "app")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "live"), []byte("actual bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	tree := SizeTreeFromSnapshot(VolumeFromMountPoint(root), []TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: filepath.Join(dir, "live"), Size: 424242}})
	ctx := WithSizeIndexes(context.Background(), []*SizeTree{tree})
	if got := allocatedResidualSize(ctx, dir); got != 424242 {
		t.Errorf("residual walked instead of using covered index: %d", got)
	}
	if got, _ := measureAppDirectory(ctx, dir); got != 424242 {
		t.Errorf("app walked instead of using covered index: %d", got)
	}
	tree.SetBlockedSubtrees([]string{dir})
	if got := allocatedResidualSize(ctx, dir); got == 424242 || got == 0 {
		t.Errorf("blocked residual did not use live fallback: %d", got)
	}
	if got, _ := measureAppDirectory(ctx, dir); got != uint64(len("actual bytes")) {
		t.Errorf("blocked app did not use live fallback: %d", got)
	}
	tree.SetBlockedSubtrees(nil)
	link := filepath.Join(root, "link")
	if err := os.Symlink(dir, link); err == nil {
		linkTree := SizeTreeFromSnapshot(VolumeFromMountPoint(root), []TreeSnapshotEntry{{Path: link, IsDir: true}, {Path: filepath.Join(link, "live"), Size: 123456}})
		linkCtx := WithSizeIndexes(context.Background(), []*SizeTree{linkTree})
		if _, ok := indexedDirectorySize(linkCtx, link); ok {
			t.Error("symlink was accepted as a cached directory")
		}
	}
	if err := os.RemoveAll(dir); err != nil {
		t.Fatal(err)
	}
	if got := allocatedResidualSize(ctx, dir); got != 0 {
		t.Errorf("missing residual trusted stale index: %d", got)
	}
	if got, _ := measureAppDirectory(ctx, dir); got != 0 {
		t.Errorf("missing app trusted stale index: %d", got)
	}
}

func TestResidualVerificationUsesLiveSize(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "residual")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "live"), []byte("data"), 0600); err != nil {
		t.Fatal(err)
	}
	tree := SizeTreeFromSnapshot(VolumeFromMountPoint(root), []TreeSnapshotEntry{{Path: dir, IsDir: true}, {Path: filepath.Join(dir, "live"), Size: 424242}})
	ctx := WithSizeIndexes(context.Background(), []*SizeTree{tree})
	item := CertainResidual(ResidualKind{Type: ResidualDirectory, Path: dir}, ResidualCacheDir)
	remaining, err := verifyMacResidualsWith(ctx, []ResidualItem{item}, nil, true)
	if err != nil || len(remaining) != 1 || remaining[0].Size() == 424242 || remaining[0].Size() == 0 {
		t.Fatalf("verification trusted old index: %+v err=%v", remaining, err)
	}
}
