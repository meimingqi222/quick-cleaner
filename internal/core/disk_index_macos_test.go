//go:build darwin || linux

package core

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type macIndexFixture struct {
	mu          sync.Mutex
	current     uint64
	changes     *platform.FSEventChanges
	replayError error
	scanError   error
	forbidFull  bool
	afterScanID uint64
	started     chan struct{}
	allowScan   <-chan struct{}
}

func (s *macIndexFixture) Current() (uint64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.current, nil
}
func (s *macIndexFixture) Changes(ctx context.Context, root string, since uint64, cache string) (*platform.FSEventChanges, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.replayError != nil {
		return nil, s.replayError
	}
	if s.changes == nil {
		return &platform.FSEventChanges{LastEventID: s.current}, nil
	}
	copy := *s.changes
	return &copy, nil
}
func (s *macIndexFixture) Scan(ctx context.Context, volume VolumeID, few bool) (ScanResult, error) {
	s.mu.Lock()
	err := s.scanError
	forbid := s.forbidFull && !few
	started, allow := s.started, s.allowScan
	s.mu.Unlock()
	if forbid {
		return ScanResult{}, errors.New("forbidden full rebuild")
	}
	if err != nil {
		return ScanResult{}, err
	}
	if started != nil {
		select {
		case started <- struct{}{}:
		default:
		}
	}
	if allow != nil {
		select {
		case <-allow:
		case <-ctx.Done():
			return ScanResult{}, ctx.Err()
		}
	}
	var entries []TreeSnapshotEntry
	err = filepath.WalkDir(volume.Mount, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		info, err := os.Lstat(path)
		if err != nil {
			return err
		}
		var size uint64
		if info.Mode().IsRegular() {
			size = uint64(info.Sys().(*syscall.Stat_t).Blocks) * 512
		}
		entries = append(entries, TreeSnapshotEntry{Path: path, IsDir: info.IsDir(), Size: size, Mtime: uint64(max(int64(0), info.ModTime().Unix()))})
		return ctx.Err()
	})
	if err != nil {
		return ScanResult{}, err
	}
	scan := ScanResult{Volume: volume, Tree: SizeTreeFromSnapshot(volume, entries)}
	refreshDiskScanTotals(&scan)
	s.mu.Lock()
	if s.afterScanID != 0 {
		s.current = s.afterScanID
	}
	s.mu.Unlock()
	return scan, nil
}

func macIndexWrite(t *testing.T, path string, n int) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, slices.Repeat([]byte{0x5a}, n), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestMacDiskIndexCheckpointReplayAndSharedSnapshot(t *testing.T) {
	ctx := context.Background()
	root := diskCacheVolumeDir(t)
	for _, rel := range []string{"direct.bin", "hot/sub/gone.bin", "hot/keep.bin", "hot-x/retain.bin", "deleted/d.bin"} {
		macIndexWrite(t, filepath.Join(root, rel), 4096)
	}
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	source := &macIndexFixture{current: 100, afterScanID: 120}
	owner := newMacDiskIndex(ctx, cache, source)
	defer owner.Close()
	volume := VolumeFromMountPoint(root)
	original, err := owner.Load(ctx, volume)
	if err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil || loaded.LastEventID != 100 || loaded.Scan.FileCount != 5 {
		t.Fatalf("full scan must save its pre-walk checkpoint: %+v, %v", loaded, err)
	}
	quiet, err := owner.Load(ctx, volume)
	if err != nil || quiet != original {
		t.Fatalf("unchanged views must share the same immutable tree: %p, %p, %v", original, quiet, err)
	}
	loaded, err = cache.Load(volume)
	if err != nil || loaded.LastEventID != 120 {
		t.Fatalf("quiet replay must persist progress: %+v, %v", loaded, err)
	}
	if err := os.Remove(filepath.Join(root, "hot/sub/gone.bin")); err != nil {
		t.Fatal(err)
	}
	if err := os.RemoveAll(filepath.Join(root, "deleted")); err != nil {
		t.Fatal(err)
	}
	macIndexWrite(t, filepath.Join(root, "hot/sub/new.bin"), 8192)
	macIndexWrite(t, filepath.Join(root, "direct.bin"), 12288)
	source.mu.Lock()
	source.current, source.afterScanID, source.forbidFull = 130, 0, true
	source.changes = &platform.FSEventChanges{
		Paths:      []string{root, filepath.Join(root, "direct.bin"), filepath.Join(root, "deleted"), filepath.Join(root, "hot-x")},
		MustRescan: []string{filepath.Join(root, "hot/sub"), filepath.Join(root, "hot")}, LastEventID: 130, Incomplete: true,
	}
	source.mu.Unlock()
	updated, err := owner.Load(ctx, volume)
	if err != nil {
		t.Fatal(err)
	}
	if updated.FileCount != 4 || updated.Tree.CountUsedFiles() != 4 || updated == original {
		t.Fatalf("partial replay must update without duplicate roots: %+v", updated)
	}
	for _, rel := range []string{"hot/sub/new.bin", "hot-x/retain.bin", "direct.bin"} {
		if _, ok := updated.Tree.FindNodeByPath(filepath.Join(root, rel)); !ok {
			t.Fatalf("missing updated/preserved path %s", rel)
		}
	}
	for _, rel := range []string{"hot/sub/gone.bin", "deleted"} {
		if _, ok := updated.Tree.FindNodeByPath(filepath.Join(root, rel)); ok {
			t.Fatalf("deleted path resurrected: %s", rel)
		}
	}
	if _, ok := original.Tree.FindNodeByPath(filepath.Join(root, "hot/sub/gone.bin")); !ok || original.FileCount != 5 {
		t.Fatal("incremental publication mutated a reader's previous snapshot")
	}
	var want uint64
	if err := filepath.Walk(root, func(_ string, info os.FileInfo, err error) error {
		if err == nil && info.Mode().IsRegular() {
			want += uint64(info.Sys().(*syscall.Stat_t).Blocks) * 512
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if updated.TotalSize != want {
		t.Fatalf("allocated bytes after mixed file/directory changes: %d != %d", updated.TotalSize, want)
	}
	loaded, err = cache.Load(volume)
	if err != nil || loaded.LastEventID != 130 || loaded.Scan.TotalSize != want || loaded.Scan.FileCount != 4 {
		t.Fatalf("replayed snapshot did not persist at delivered prefix: %+v, %v", loaded, err)
	}
}

func TestMacDiskIndexBackgroundNeverRebuilds(t *testing.T) {
	for _, failure := range []string{"unavailable", "wrapped", "root-rescan", "stale", "subtree-failure"} {
		t.Run(failure, func(t *testing.T) {
			root := diskCacheVolumeDir(t)
			macIndexWrite(t, filepath.Join(root, "sub/data"), 4096)
			cache, err := NewDiskIndexCache(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			source := &macIndexFixture{current: 10}
			owner := newMacDiskIndex(context.Background(), cache, source)
			defer owner.Close()
			if advanced, err := owner.Advance(context.Background()); err != nil || advanced {
				t.Fatalf("never-loaded index must remain idle: %v, %v", advanced, err)
			}
			volume := VolumeFromMountPoint(root)
			if _, err := owner.Load(context.Background(), volume); err != nil {
				t.Fatal(err)
			}
			source.forbidFull, source.current = true, 11
			source.changes = &platform.FSEventChanges{LastEventID: 11}
			switch failure {
			case "unavailable":
				source.replayError = errors.New("no history")
			case "wrapped":
				source.changes.RequiresFullScan = true
			case "root-rescan":
				source.changes.MustRescan = []string{root}
			case "stale":
				source.current = 4_000_011
			case "subtree-failure":
				source.changes.Paths = []string{filepath.Join(root, "sub")}
				source.scanError = errors.New("unreadable subtree")
			}
			if advanced, err := owner.Advance(context.Background()); advanced || !errors.Is(err, ErrDiskIndexRebuildNeeded) {
				t.Fatalf("background must defer full scan: advanced=%v, error=%v", advanced, err)
			}
			loaded, err := cache.Load(volume)
			if err != nil || loaded.LastEventID != 10 || loaded.Scan.FileCount != 1 {
				t.Fatalf("failed replay must leave durable snapshot and ID unchanged: %+v, %v", loaded, err)
			}
		})
	}
}

func TestMacDiskIndexCancellationAndNoBackgroundContention(t *testing.T) {
	cache, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	source := &macIndexFixture{current: 5, started: make(chan struct{}, 1), allowScan: make(chan struct{})}
	owner := newMacDiskIndex(context.Background(), cache, source)
	root := t.TempDir()
	done := make(chan error, 1)
	go func() { _, err := owner.Load(context.Background(), VolumeFromMountPoint(root)); done <- err }()
	select {
	case <-source.started:
	case <-time.After(3 * time.Second):
		t.Fatal("scan did not begin")
	}
	if advanced, err := owner.Advance(context.Background()); advanced || err != nil {
		t.Fatalf("background must skip a busy user scan: %v, %v", advanced, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := owner.Load(ctx, VolumeFromMountPoint(root)); !errors.Is(err, context.Canceled) {
		t.Fatalf("waiting load must respect cancellation: %v", err)
	}
	owner.Close()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("close did not cancel acquisition")
	}
	if _, err := cache.Load(VolumeFromMountPoint(root)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("cancelled full scan must not publish: %v", err)
	}
	if _, err := owner.Load(context.Background(), VolumeFromMountPoint(root)); !errors.Is(err, context.Canceled) {
		t.Fatalf("closed owner must stay closed: %v", err)
	}
}

func TestMacDiskIndexRawCoverageAndMetadataErrors(t *testing.T) {
	root := t.TempDir()
	cloud := filepath.Join(root, "cloud")
	if err := os.Mkdir(cloud, 0o700); err != nil {
		t.Fatal(err)
	}
	raw := platform.MacWalkResult{Volume: VolumeFromMountPoint(root), Entries: []platform.MacWalkEntry{
		{Used: true, IsDir: true}, {Parent: 0, Name: "cloud", Used: true, IsDir: true},
		{Parent: 0, Name: "direct", Used: true, IsRegular: true, SizeKnown: true, Size: 8192, Mtime: 44},
	}, Incomplete: []platform.MacWalkCoverage{{Index: 1, Path: cloud, Reason: platform.MacWalkDataless}}}
	scan, err := BuildMacWalkScanResult(context.Background(), raw)
	if err != nil || scan.TotalSize != 8192 || scan.FileCount != 1 || scan.Tree.CoversSubtree(cloud) {
		t.Fatalf("raw coverage/allocated accounting lost: %+v, %v", scan, err)
	}
	source := &macIndexFixture{current: 2}
	updated, err := refreshMacDiskIndex(context.Background(), scan, &platform.FSEventChanges{MustRescan: []string{cloud}}, source)
	if err != nil || !updated.Tree.CoversSubtree(cloud) || scan.Tree.CoversSubtree(cloud) {
		t.Fatalf("materialized subtree must heal only the new snapshot's coverage: %+v, %v", updated, err)
	}
	invalid := filepath.Join(root, "invalid") + string([]byte{0}) + "name"
	updated, err = refreshMacDiskIndex(context.Background(), scan, &platform.FSEventChanges{Paths: []string{invalid}}, source)
	if err != nil || updated.TotalSize != 8192 || !reflect.DeepEqual(updated.Tree.blocked, []string{cloud, invalid}) {
		t.Fatalf("metadata failure must retain old data and unknown coverage: %+v, %v", updated, err)
	}
	// Component boundaries must not swallow a-x or rescan a/b twice.
	roots := coalesceMacIndexRoots([]string{filepath.Join(root, "a", "b"), filepath.Join(root, "a-x"), filepath.Join(root, "a")})
	if len(roots) != 2 || !slices.Contains(roots, filepath.Join(root, "a")) || !slices.Contains(roots, filepath.Join(root, "a-x")) {
		t.Fatalf("component-wise root collapse: %v", roots)
	}
}

type readOnlyIndexSource struct{}

func (readOnlyIndexSource) Current() (uint64, error) { panic("cached lookup read events") }
func (readOnlyIndexSource) Changes(context.Context, string, uint64, string) (*platform.FSEventChanges, error) {
	panic("cached lookup replayed events")
}
func (readOnlyIndexSource) Scan(context.Context, VolumeID, bool) (ScanResult, error) {
	panic("cached lookup started a scan")
}

func TestMacDiskIndexCachedNeverStartsOrReplaysScan(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	if _, err := cache.Save(volume, tree, 37); err != nil {
		t.Fatal(err)
	}
	owner := newMacDiskIndex(context.Background(), cache, readOnlyIndexSource{})
	defer owner.Close()
	if owner.Peek(volume) != nil {
		t.Fatal("peek must not load a cold cache")
	}
	first, err := owner.Cached(context.Background(), volume)
	if err != nil || first == nil || first.Tree == nil {
		t.Fatalf("existing snapshot not loaded: %v", err)
	}
	if owner.Peek(volume) != first {
		t.Fatal("peek missed the loaded snapshot")
	}
	owner.gate <- struct{}{}
	if owner.Peek(volume) != nil {
		t.Fatal("peek waited for a busy owner")
	}
	<-owner.gate
	second, err := owner.Cached(context.Background(), volume)
	if err != nil || second != first {
		t.Fatalf("snapshot was not reused: %v", err)
	}
	missing := newMacDiskIndex(context.Background(), cache, readOnlyIndexSource{})
	defer missing.Close()
	if _, err := missing.Cached(context.Background(), VolumeFromMountPoint(filepath.Join(volume.Mount, "missing"))); err == nil {
		t.Fatal("missing index did not report fallback")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := owner.Cached(ctx, volume); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled lookup: %v", err)
	}
}
