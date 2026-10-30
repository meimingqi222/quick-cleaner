//go:build darwin || linux

package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync/atomic"
	"syscall"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type permissionIndexSource struct {
	*macIndexFixture
	denied    string
	denyErr   error
	fullScans atomic.Int64
}

func (s *permissionIndexSource) Scan(ctx context.Context, volume VolumeID, few bool) (ScanResult, error) {
	if !few {
		s.fullScans.Add(1)
		return ScanResult{}, errors.New("unexpected full-volume rebuild")
	}
	if volume.Mount == s.denied && s.denyErr != nil {
		return ScanResult{}, &os.PathError{Op: "scan", Path: volume.Mount, Err: s.denyErr}
	}
	return s.macIndexFixture.Scan(ctx, volume, few)
}

func TestMacDiskIndexPermissionFailureKeepsUnknownSubtreeWithoutRebuild(t *testing.T) {
	for _, test := range []struct {
		name string
		err  error
	}{{"EPERM", syscall.EPERM}, {"EACCES", syscall.EACCES}, {"corruption", errors.New("malformed scan")}} {
		t.Run(test.name, func(t *testing.T) {
			root := diskCacheVolumeDir(t)
			denied, healthy := filepath.Join(root, "denied"), filepath.Join(root, "healthy")
			for _, dir := range []string{denied, healthy} {
				if err := os.MkdirAll(dir, 0700); err != nil {
					t.Fatal(err)
				}
			}
			if err := os.WriteFile(filepath.Join(denied, "keep"), []byte("keep"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(healthy, "new"), []byte("new"), 0600); err != nil {
				t.Fatal(err)
			}
			volume := VolumeFromMountPoint(root)
			original := SizeTreeFromSnapshot(volume, []TreeSnapshotEntry{{Path: denied, IsDir: true}, {Path: filepath.Join(denied, "keep"), Size: 123}, {Path: healthy, IsDir: true}, {Path: filepath.Join(healthy, "old"), Size: 7}})
			cache, err := NewDiskIndexCache(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			if _, err = cache.Save(volume, original, 1); err != nil {
				t.Fatal(err)
			}
			source := &permissionIndexSource{macIndexFixture: &macIndexFixture{current: 2, changes: &platform.FSEventChanges{Paths: []string{denied, healthy}, LastEventID: 2}}, denied: denied, denyErr: test.err}
			owner := newMacDiskIndex(context.Background(), cache, source)
			defer func() { owner.Close(); flushMacIndex(t, owner) }()
			scan, err := owner.Load(context.Background(), volume)
			if test.name == "corruption" {
				if err == nil || source.fullScans.Load() != 1 {
					t.Fatal("unexpected subtree corruption no longer requests a rebuild")
				}
				return
			}
			if err != nil || source.fullScans.Load() != 0 {
				t.Fatalf("local permission error triggered a full rebuild: %v", err)
			}
			if scan.Tree.CoversSubtree(denied) || !scan.Tree.CoversSubtree(healthy) {
				t.Fatal("permission failure lost unknown coverage or contaminated healthy subtree")
			}
			if _, ok := scan.Tree.FindNodeByPath(filepath.Join(denied, "keep")); !ok {
				t.Fatal("unreadable subtree was erased as if absent")
			}
			if _, ok := scan.Tree.FindNodeByPath(filepath.Join(healthy, "new")); !ok {
				t.Fatal("healthy subtree was not refreshed")
			}
			if _, ok := scan.Tree.FindNodeByPath(filepath.Join(healthy, "old")); ok {
				t.Fatal("healthy stale subtree remained")
			}
			if !original.CoversSubtree(denied) {
				t.Fatal("refresh mutated the original coverage")
			}
			flushMacIndex(t, owner)
			saved, err := cache.Load(volume)
			if err != nil || saved.Scan.Tree.CoversSubtree(denied) {
				t.Fatalf("unknown coverage did not persist: %v", err)
			}
			source.denyErr = nil
			source.current = 3
			source.changes = &platform.FSEventChanges{Paths: []string{denied}, LastEventID: 3}
			recovered, err := owner.Load(context.Background(), volume)
			if err != nil || !recovered.Tree.CoversSubtree(denied) || scan.Tree.CoversSubtree(denied) {
				t.Fatalf("recovery did not heal only the new snapshot: %v", err)
			}
		})
	}
}
