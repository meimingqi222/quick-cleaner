//go:build darwin || linux

package core

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type compactingIndexSource struct {
	*macIndexFixture
	subtree *SizeTree
}

func (s *compactingIndexSource) Scan(_ context.Context, _ VolumeID, few bool) (ScanResult, error) {
	if !few {
		return ScanResult{}, fmt.Errorf("unexpected full scan")
	}
	return ScanResult{Tree: s.subtree}, nil
}

func TestMacDiskIndexCompactionRebindsMappedBase(t *testing.T) {
	cache, volume, tree := diskCacheFixture(t)
	project := filepath.Join(volume.Mount, "project")
	if err := os.MkdirAll(project, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := cache.Save(volume, tree, 1); err != nil {
		t.Fatal(err)
	}
	entries := []TreeIndexEntry{{Name: "project", Used: true, IsDir: true}}
	for i := 0; i <= DiskIndexDeltaCompactThreshold/2; i++ {
		entries = append(entries, TreeIndexEntry{Parent: 0, Name: fmt.Sprintf("file-%d", i), Used: true, Size: 1})
	}
	source := &compactingIndexSource{macIndexFixture: &macIndexFixture{current: 2, changes: &platform.FSEventChanges{Paths: []string{project}, LastEventID: 2}}, subtree: NewSizeTree(VolumeFromMountPoint(project), entries)}
	owner := newMacDiskIndex(context.Background(), cache, source)
	defer func() { owner.Close(); flushMacIndex(t, owner) }()
	first, err := owner.Load(context.Background(), volume)
	if err != nil {
		t.Fatal(err)
	}
	base, delta := cache.Paths(volume)
	if _, err = os.Stat(delta); !os.IsNotExist(err) {
		t.Fatal("fixture did not trigger compaction", err)
	}
	flushMacIndex(t, owner)
	disk, err := cache.Load(volume)
	if err != nil {
		t.Fatal(err)
	}
	current := owner.Peek(volume)
	if first.Tree.FileCountOf(0) != current.Tree.FileCountOf(0) {
		t.Fatal("rebind changed the published scan")
	}
	if current.Tree.base == nil || cacheU64(current.Tree.base.header(), 72) != disk.BaseChecksum {
		t.Error("owner still maps the retired base after compaction")
	}
	before := diskCacheRead(t, base)
	source.current = 3
	source.changes = &platform.FSEventChanges{LastEventID: 3}
	if _, err = owner.Load(context.Background(), volume); err != nil {
		t.Fatal(err)
	}
	flushMacIndex(t, owner)
	if !bytes.Equal(before, diskCacheRead(t, base)) {
		t.Error("quiet refresh rewrote the entire compacted base")
	}
	if _, err = os.Stat(delta); err != nil {
		t.Error("quiet refresh did not use a metadata-only delta", err)
	}
}
