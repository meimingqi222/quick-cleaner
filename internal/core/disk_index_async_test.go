//go:build darwin || linux

package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type pausedIndexCache struct {
	*DiskIndexCache
	started, release chan struct{}
	once             sync.Once
}

func (c *pausedIndexCache) PersistIncremental(v VolumeID, tree *SizeTree, event uint64) (DiskIndexSaveResult, error) {
	c.once.Do(func() { close(c.started); <-c.release })
	return c.DiskIndexCache.PersistIncremental(v, tree, event)
}

func TestMacDiskIndexSlowPersistenceDoesNotBlockReaders(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	source := &macIndexFixture{current: 2, changes: &platform.FSEventChanges{LastEventID: 2}}
	owner := newMacDiskIndex(context.Background(), cache, source)
	paused := &pausedIndexCache{DiskIndexCache: cache, started: make(chan struct{}), release: make(chan struct{})}
	owner.writer = paused
	var release sync.Once
	unpause := func() { release.Do(func() { close(paused.release) }) }
	defer func() { unpause(); owner.Flush(context.Background()); owner.Close() }()
	done := make(chan error, 1)
	go func() { _, err := owner.Load(context.Background(), volume); done <- err }()
	select {
	case <-paused.started:
	case <-time.After(5 * time.Second):
		t.Fatal("save did not start")
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		unpause()
		<-done
		t.Fatal("Load waited for slow persistence before publishing its snapshot")
	}
	if owner.Peek(volume) == nil {
		t.Fatal("save holds the reader gate")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err := owner.Flush(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("Flush must wait for durable work: %v", err)
	}
	// Two more snapshots arrive while the first save is blocked. Only the
	// newest pending checkpoint is needed; none may overwrite newer state.
	path := filepath.Join(volume.Mount, "project", "new.bin")
	macIndexWrite(t, path, 4096)
	source.current = 3
	source.changes = &platform.FSEventChanges{Paths: []string{path}, LastEventID: 3}
	if _, err := owner.Load(context.Background(), volume); err != nil {
		t.Fatal(err)
	}
	source.current = 4
	source.changes = &platform.FSEventChanges{LastEventID: 4}
	if _, err := owner.Load(context.Background(), volume); err != nil {
		t.Fatal(err)
	}
	owner.Close()
	unpause()
	if err := owner.Flush(context.Background()); err != nil {
		t.Fatal(err)
	}
	loaded, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.LastEventID != 4 {
		t.Fatalf("saved checkpoint %d, want 4", loaded.LastEventID)
	}
	if _, ok := loaded.Scan.Tree.FindNodeByPath(path); !ok {
		t.Fatal("newest snapshot was lost")
	}
}

func flushMacIndex(t *testing.T, owner *MacDiskIndex) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	if err := owner.Flush(ctx); err != nil {
		t.Fatal(err)
	}
}

type failingIndexWriter struct{ *DiskIndexCache }

func (c failingIndexWriter) PersistIncremental(VolumeID, *SizeTree, uint64) (DiskIndexSaveResult, error) {
	return DiskIndexSaveResult{}, os.ErrPermission
}

func TestMacDiskIndexFlushReportsFailureAndPreservesSnapshot(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	source := &macIndexFixture{current: 2, changes: &platform.FSEventChanges{LastEventID: 2}}
	owner := newMacDiskIndex(context.Background(), cache, source)
	defer owner.Close()
	owner.writer = failingIndexWriter{cache}
	scan, err := owner.Load(context.Background(), volume)
	if err != nil {
		t.Fatal(err)
	}
	if err := owner.Flush(context.Background()); !errors.Is(err, os.ErrPermission) {
		t.Fatalf("lost save error: %v", err)
	}
	if owner.Peek(volume) != scan {
		t.Fatal("failed save replaced the readable snapshot")
	}
	loaded, err := cache.Load(volume)
	if err != nil || loaded.LastEventID != 1 {
		t.Fatal("failed save changed durable checkpoint", err)
	}
}
