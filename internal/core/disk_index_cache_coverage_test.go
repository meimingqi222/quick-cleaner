package core

import (
	"bytes"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestDiskIndexCacheCoverageSurvivesReloadDeltaAndCompaction(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	blocked := filepath.Join(v.Mount, "project", "node_modules")
	cloud := filepath.Join(v.Mount, "unlisted-cloud")
	tree.SetBlockedSubtrees([]string{cloud, blocked, cloud})
	if _, err := c.Save(v, tree, 1); err != nil {
		t.Fatal(err)
	}
	check := func(event uint64) *LoadedDiskIndex {
		t.Helper()
		l, e := c.Load(v)
		if e != nil {
			t.Fatal(e)
		}
		if l.LastEventID != event || !reflect.DeepEqual(l.Scan.Tree.BlockedSubtrees(), []string{blocked, cloud}) {
			t.Fatalf("coverage lost: %+v %v", l, l.Scan.Tree.BlockedSubtrees())
		}
		for _, p := range []string{v.Mount, blocked, filepath.Join(cloud, "missing.bin")} {
			if l.Scan.Tree.CoversSubtree(p) {
				t.Fatalf("unknown subtree treated as complete: %s", p)
			}
		}
		if !l.Scan.Tree.CoversSubtree(filepath.Join(v.Mount, "project", "keep.bin")) {
			t.Fatal("unrelated known coverage lost")
		}
		return l
	}
	l := check(1)
	base, delta := c.Paths(v)
	oldBase := diskCacheRead(t, base)
	r, e := c.PersistIncremental(v, l.Scan.Tree, 2)
	if e != nil || !r.Delta {
		t.Fatalf("metadata delta %+v %v", r, e)
	}
	if len(diskCacheRead(t, delta)) != 128 || !bytes.Equal(oldBase, diskCacheRead(t, base)) {
		t.Fatal("metadata-only delta rewrote payload")
	}
	l = check(2)
	// Change only coverage: unchanged slots must still get a newly bound delta.
	l.Scan.Tree.SetBlockedSubtrees([]string{cloud})
	if _, e = c.PersistIncremental(v, l.Scan.Tree, 3); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || !l.Scan.Tree.CoversSubtree(blocked) || l.Scan.Tree.CoversSubtree(v.Mount) {
		t.Fatal("coverage-only update lost", e)
	}
	l.Scan.Tree.SetBlockedSubtrees([]string{blocked, cloud})
	if _, e = c.Save(v, l.Scan.Tree, 4); e != nil {
		t.Fatal(e)
	}
	check(4)
	if _, e = os.Stat(delta); !errors.Is(e, os.ErrNotExist) {
		t.Fatal("full compaction retained delta")
	}
	// Force automatic threshold compaction with heap-appended slots.
	l, _ = c.Load(v)
	for i := 0; i < DiskIndexDeltaCompactThreshold/2+1; i++ {
		l.Scan.Tree.entries = append(l.Scan.Tree.entries, treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: 0, Name: "extra", Used: true, Size: 1}})
	}
	l.Scan.Tree.RebuildChildArrays()
	l.Scan.Tree.RecomputeAggregates()
	r, e = c.PersistIncremental(v, l.Scan.Tree, 5)
	if e != nil || !r.Compacted {
		t.Fatalf("automatic compaction %+v %v", r, e)
	}
	check(5)
}

func TestDiskIndexCacheCoverageRejectsCorruptionAndBindingMismatch(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "cloud")})
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	base, delta := c.Paths(v)
	baseBytes := diskCacheRead(t, base)
	baseCov := diskIndexCoveragePath(base, cacheU64(baseBytes, 72))
	good := diskCacheRead(t, baseCov)
	mutations := map[string]func(*diskIndexCoverage){
		"base checksum":        func(c *diskIndexCoverage) { c.BaseChecksum++ },
		"delta checksum":       func(c *diskIndexCoverage) { c.DeltaChecksum++ },
		"watermark":            func(c *diskIndexCoverage) { c.LastEventID++ },
		"version":              func(c *diskIndexCoverage) { c.Version++ },
		"removed blocked root": func(c *diskIndexCoverage) { c.Blocked = []string{} },
		"outside volume":       func(c *diskIndexCoverage) { c.Blocked = []string{filepath.Dir(v.Mount)} },
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			var cov diskIndexCoverage
			if e := json.Unmarshal(good, &cov); e != nil {
				t.Fatal(e)
			}
			mutate(&cov)
			b, e := json.Marshal(cov)
			if e != nil {
				t.Fatal(e)
			}
			diskCacheWrite(t, baseCov, b)
			if _, e = c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
				t.Fatalf("mismatched coverage accepted: %v", e)
			}
		})
	}
	for _, bad := range [][]byte{nil, []byte("{"), append(bytes.Clone(good), 'x')} {
		diskCacheWrite(t, baseCov, bad)
		if _, e := c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
			t.Fatalf("corrupt coverage accepted: %v", e)
		}
	}
	if e := os.Remove(baseCov); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatalf("required coverage missing: %v", e)
	}
	diskCacheWrite(t, baseCov, good)
	l, _ := c.Load(v)
	l.Scan.Tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "project")})
	if _, e := c.PersistIncremental(v, l.Scan.Tree, 2); e != nil {
		t.Fatal(e)
	}
	deltaBytes := diskCacheRead(t, delta)
	deltaCov := diskIndexCoveragePath(delta, cacheU64(deltaBytes, 100))
	goodDelta := diskCacheRead(t, deltaCov)
	for _, bad := range [][]byte{nil, good, []byte("broken")} {
		diskCacheWrite(t, deltaCov, bad)
		restored, e := c.Load(v)
		if e != nil || restored.DeltaApplied || !errors.Is(restored.DeltaError, ErrDiskIndexIncomplete) || restored.LastEventID != 1 || !reflect.DeepEqual(restored.Scan.Tree.BlockedSubtrees(), tree.BlockedSubtrees()) {
			t.Fatalf("rejected delta coverage leaked state: %+v %v", restored, e)
		}
	}
	if e := os.Remove(deltaCov); e != nil {
		t.Fatal(e)
	}
	l, e := c.Load(v)
	if e != nil || l.DeltaApplied || l.LastEventID != 1 || l.Scan.Tree.CoversSubtree(v.Mount) {
		t.Fatal("missing delta coverage became complete", e)
	}
	diskCacheWrite(t, deltaCov, goodDelta)
	l, e = c.Load(v)
	if e != nil || !l.DeltaApplied || l.LastEventID != 2 {
		t.Fatal("valid delta coverage failed", e)
	}
}

func TestDiskIndexCacheCoveragePublicationBoundariesAndRollback(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "old-cloud")})
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	base, _ := c.Paths(v)
	old := diskCacheRead(t, base)
	newTree := tree.Clone()
	newTree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "new-cloud")})
	meta := DiskIndexMetadata{LastEventID: 2, FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	next, e := encodeDiskIndex(newTree, v, meta)
	if e != nil {
		t.Fatal(e)
	}
	covPath, cov, e := prepareDiskIndexCoverage(base, next, newTree, 0, 2, false)
	if e != nil {
		t.Fatal(e)
	}
	// A failed coverage publication cannot replace the previous index or coverage.
	if e = os.Mkdir(covPath, 0700); e != nil {
		t.Fatal(e)
	}
	if e = publishDiskIndexCovered(base, next, newTree, 0, 2, false); e == nil {
		t.Fatal("coverage publication failure reported success")
	}
	l, e := c.Load(v)
	if e != nil || l.LastEventID != 1 || !reflect.DeepEqual(l.Scan.Tree.BlockedSubtrees(), tree.BlockedSubtrees()) || !bytes.Equal(old, diskCacheRead(t, base)) {
		t.Fatal("failed publication damaged old pair", e)
	}
	if e = os.Remove(covPath); e != nil {
		t.Fatal(e)
	}
	// Crash after coverage publication, before index rename: old pair remains.
	if e = publishDiskIndex(covPath, cov); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || l.LastEventID != 1 {
		t.Fatal("staged coverage changed index", e)
	}
	// Index publication selects only its exact sidecar.
	if e = publishDiskIndex(base, next); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || l.LastEventID != 2 || !reflect.DeepEqual(l.Scan.Tree.BlockedSubtrees(), newTree.BlockedSubtrees()) {
		t.Fatal("new pair inconsistent", e)
	}
	// Explicit rollback to retained old bytes selects retained old coverage.
	if e = publishDiskIndex(base, old); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || l.LastEventID != 1 || !reflect.DeepEqual(l.Scan.Tree.BlockedSubtrees(), tree.BlockedSubtrees()) {
		t.Fatal("rollback coverage mixed generations", e)
	}
	// Simulate an index arriving without its required sidecar: fail closed.
	if e = os.Remove(covPath); e != nil {
		t.Fatal(e)
	}
	if e = publishDiskIndex(base, next); e != nil {
		t.Fatal(e)
	}
	if _, e = c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatalf("partially published pair accepted: %v", e)
	}
}

func TestDiskIndexCacheCoverageEqualWatermarkRewriteInvalidatesOldDelta(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	l, _ := c.Load(v)
	l.Scan.Tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "cloud")})
	if _, e := c.PersistIncremental(v, l.Scan.Tree, 1); e != nil {
		t.Fatal(e)
	}
	base, delta := c.Paths(v)
	oldDelta := diskCacheRead(t, delta)
	oldBase := diskCacheRead(t, base)
	// Identical metadata (including scan time) still needs a fresh base identity.
	meta := DiskIndexMetadata{LastEventID: 1, ScannedAt: cacheU64(oldBase, 64), FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	b, e := encodeDiskIndex(tree, v, meta)
	if e != nil {
		t.Fatal(e)
	}
	if e = publishDiskIndexCovered(base, b, tree, 0, 1, false); e != nil {
		t.Fatal(e)
	}
	if cacheU64(oldBase, 72) == cacheU64(b, 72) {
		t.Fatal("full rewrite reused base generation")
	}
	if !bytes.Equal(oldDelta, diskCacheRead(t, delta)) {
		t.Fatal("test did not preserve crash-window stale delta")
	}
	l, e = c.Load(v)
	if e != nil || l.DeltaApplied || l.DeltaError == nil || !l.Scan.Tree.CoversSubtree(v.Mount) {
		t.Fatal("old delta applied to replacement base", e)
	}
}

func TestDiskIndexCacheCoverageLegacyAndLinksFailClosed(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	meta := DiskIndexMetadata{LastEventID: 1, FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	b, e := encodeDiskIndex(tree, v, meta)
	if e != nil {
		t.Fatal(e)
	}
	base, _ := c.Paths(v)
	diskCacheWrite(t, base, b)
	l, e := c.Load(v)
	if e != nil || l.Scan.Tree.CoversSubtree(v.Mount) || !reflect.DeepEqual(l.Scan.Tree.BlockedSubtrees(), []string{v.Mount}) {
		t.Fatal("legacy coverage invented", e)
	}
	if _, e = c.Save(v, tree, 2); e != nil {
		t.Fatal(e)
	}
	b = diskCacheRead(t, base)
	covPath := diskIndexCoveragePath(base, cacheU64(b, 72))
	saved := diskCacheRead(t, covPath)
	outside := filepath.Join(t.TempDir(), "coverage")
	diskCacheWrite(t, outside, saved)
	if e = os.Remove(covPath); e != nil {
		t.Fatal(e)
	}
	if e = os.Symlink(outside, covPath); e != nil {
		t.Skipf("symlinks unavailable: %v", e)
	}
	if _, e = c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatal("coverage symlink followed", e)
	}
	if !bytes.Equal(saved, diskCacheRead(t, outside)) {
		t.Fatal("symlink target changed")
	}
	invalid := tree.Clone()
	invalid.SetBlockedSubtrees([]string{filepath.Dir(v.Mount)})
	if _, e = c.Save(v, invalid, 3); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatal("outside coverage accepted", e)
	}
}

func TestDiskIndexCacheCoverageDeltaStagingAndMissingBaseRepair(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "base-cloud")})
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	l, _ := c.Load(v)
	l.Scan.Tree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "delta-cloud")})
	if _, e := c.PersistIncremental(v, l.Scan.Tree, 2); e != nil {
		t.Fatal(e)
	}
	base, delta := c.Paths(v)
	baseBytes := diskCacheRead(t, base)
	oldDelta := diskCacheRead(t, delta)
	original, e := decodeDiskIndex(baseBytes, v)
	if e != nil {
		t.Fatal(e)
	}
	nextTree := l.Scan.Tree.Clone()
	nextTree.SetBlockedSubtrees([]string{filepath.Join(v.Mount, "next-cloud")})
	meta := DiskIndexMetadata{LastEventID: 3, FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	next, _, e := encodeDiskIndexDelta(nextTree, original, meta)
	if e != nil {
		t.Fatal(e)
	}
	covPath, cov, e := prepareDiskIndexCoverage(delta, next, nextTree, original.checksum, 3, true)
	if e != nil {
		t.Fatal(e)
	}
	if e = publishDiskIndex(covPath, cov); e != nil {
		t.Fatal(e)
	}
	current, e := c.Load(v)
	if e != nil || current.LastEventID != 2 || !reflect.DeepEqual(current.Scan.Tree.BlockedSubtrees(), l.Scan.Tree.BlockedSubtrees()) {
		t.Fatal("staged delta sidecar changed visible coverage", e)
	}
	if e = publishDiskIndex(delta, next); e != nil {
		t.Fatal(e)
	}
	current, e = c.Load(v)
	if e != nil || current.LastEventID != 3 || !reflect.DeepEqual(current.Scan.Tree.BlockedSubtrees(), nextTree.BlockedSubtrees()) {
		t.Fatal("new delta did not select exact coverage", e)
	}
	if e = publishDiskIndex(delta, oldDelta); e != nil {
		t.Fatal(e)
	}
	current, e = c.Load(v)
	if e != nil || current.LastEventID != 2 || !reflect.DeepEqual(current.Scan.Tree.BlockedSubtrees(), l.Scan.Tree.BlockedSubtrees()) {
		t.Fatal("delta rollback mixed coverage", e)
	}
	if e = os.Remove(diskIndexCoveragePath(base, original.checksum)); e != nil {
		t.Fatal(e)
	}
	if _, e = c.Load(v); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatal("missing required base coverage accepted", e)
	}
	// A successful incremental API call must repair the unreadable base, not
	// return success after adding a delta to a base that can never be loaded.
	r, e := c.PersistIncremental(v, nextTree, 4)
	if e != nil || !r.Compacted || r.Delta {
		t.Fatalf("base repair %+v %v", r, e)
	}
	current, e = c.Load(v)
	if e != nil || current.LastEventID != 4 || !reflect.DeepEqual(current.Scan.Tree.BlockedSubtrees(), nextTree.BlockedSubtrees()) {
		t.Fatal("base repair lost coverage", e)
	}
}
