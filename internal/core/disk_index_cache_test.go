package core

import (
	"bytes"
	"encoding/binary"
	"errors"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func diskCacheFixture(t *testing.T) (*DiskIndexCache, VolumeID, *SizeTree) {
	t.Helper()
	c, err := NewDiskIndexCache(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	v := VolumeFromMountPointWithLabel(filepath.Join(diskCacheVolumeDir(t), "卷"), "测试 Disk")
	tree := NewSizeTree(v, []TreeIndexEntry{
		{Used: true, IsDir: true, Name: "/"},
		{Parent: 0, Used: true, IsDir: true, Name: "project"},
		{Parent: 1, Used: true, IsDir: true, Name: "node_modules"},
		{Parent: 2, Used: true, Name: "index.js", Size: 4096, Mtime: 123},
		{Parent: 1, Used: true, Name: "keep.bin", Size: 100, Mtime: 124},
	})
	return c, v, tree
}

func diskCacheVolumeDir(t *testing.T) string {
	t.Helper()
	// Hex mount keys must fit a filesystem filename even under macOS's long
	// temporary-directory prefix; do not also embed the entire test name.
	dir, err := os.MkdirTemp("", "qc-volume-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(dir) })
	return dir
}

func diskCacheRead(t *testing.T, path string) []byte {
	t.Helper()
	b, e := os.ReadFile(path)
	if e != nil {
		t.Fatal(e)
	}
	return b
}
func diskCacheWrite(t *testing.T, path string, b []byte) {
	t.Helper()
	if e := os.WriteFile(path, b, 0600); e != nil {
		t.Fatal(e)
	}
}

func TestLoadedIndexKeepsMappedBaseAndOverrides(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("mapped index is darwin/linux only; Windows decodes into the heap")
	}
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	loaded, e := c.Load(v)
	if e != nil {
		t.Fatal(e)
	}
	if loaded.Scan.Tree.base == nil || loaded.Scan.Tree.baseCount == 0 || len(loaded.Scan.Tree.entries) != 0 {
		t.Fatal("loaded index copied the base into heap entries")
	}
	base, _ := c.Paths(v)
	before := diskCacheRead(t, base)
	idx, ok := loaded.Scan.Tree.FindNodeByPath(filepath.Join(v.Mount, "project"))
	if !ok {
		t.Fatal("mapped base path missing")
	}
	slot := loaded.Scan.Tree.slot(idx)
	slot.Mtime = 99
	loaded.Scan.Tree.setSlot(idx, slot)
	if len(loaded.Scan.Tree.overrides) != 1 {
		t.Fatal("mapped write did not become an override", len(loaded.Scan.Tree.overrides))
	}
	if !bytes.Equal(before, diskCacheRead(t, base)) {
		t.Fatal("override wrote the shared base file")
	}
	clone := loaded.Scan.Tree.Clone()
	clone.RemoveSubtreeInplace(idx)
	if !loaded.Scan.Tree.Valid(idx) || loaded.Scan.Tree.slot(idx).Mtime != 99 {
		t.Fatal("clone shared its override map with the original")
	}
}

func TestDiskIndexCacheRoundTripAndClone(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	saved, err := c.Save(v, tree, 7)
	if err != nil {
		t.Fatal(err)
	}
	loaded, err := c.Load(v)
	if err != nil {
		t.Fatal(err)
	}
	if loaded.Scan.Tree.Root() != 0 || loaded.LastEventID != 7 || loaded.Scan.TotalSize != 4196 || loaded.FileCount != 2 || loaded.DirCount != 3 || loaded.BaseChecksum != saved.BaseChecksum {
		t.Fatalf("bad metadata: %+v", loaded)
	}
	path := filepath.Join(v.Mount, "project", "node_modules", "index.js")
	idx, ok := loaded.Scan.Tree.FindNodeByPath(path)
	if !ok || loaded.Scan.Tree.slot(idx).Mtime != 123 {
		t.Fatal("nested path/mtime lost")
	}
	if len(loaded.Scan.Tree.Search("index.js", 10)) != 1 {
		t.Fatal("nested search lost")
	}
	clone := loaded.Scan.Tree.Clone()
	clone.RemoveSubtreeInplace(idx)
	if !loaded.Scan.Tree.Valid(idx) {
		t.Fatal("clone mutated original")
	}
	base, _ := c.Paths(v)
	raw := diskCacheRead(t, base)
	if string(raw[:8]) != "QCIDXV07" || binary.LittleEndian.Uint32(raw[8:]) != 7 || binary.LittleEndian.Uint32(raw[int(cacheU32(raw, 84)):]) != 0xc0000000 {
		t.Fatal("not Rust v7 entry layout")
	}
}

func TestDiskIndexCacheDeltaCumulativeAndCompaction(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	base, delta := c.Paths(v)
	original := diskCacheRead(t, base)
	l, e := c.Load(v)
	if e != nil {
		t.Fatal(e)
	}
	modified := l.Scan.Tree.Clone()
	nm, _ := modified.FindNodeByPath(filepath.Join(v.Mount, "project", "node_modules"))
	modified.RemoveSubtreeInplace(nm)
	if !modified.UpsertFileWithMtime(filepath.Join(v.Mount, "project", "added.bin"), 777, 456) {
		t.Fatal("upsert")
	}
	r, e := c.PersistIncremental(v, modified, 5)
	if e != nil || !r.Delta {
		t.Fatalf("delta save %+v %v", r, e)
	}
	if !bytes.Equal(original, diskCacheRead(t, base)) {
		t.Fatal("small delta rewrote base")
	}
	l, e = c.Load(v)
	if e != nil || !l.DeltaApplied || l.LastEventID != 5 || l.TotalSize != 877 {
		t.Fatalf("delta load %+v %v", l, e)
	}
	if _, ok := l.Scan.Tree.FindNodeByPath(filepath.Join(v.Mount, "project", "node_modules")); ok {
		t.Fatal("tombstone resurrected")
	}
	if !l.Scan.Tree.UpsertFile(filepath.Join(v.Mount, "second.bin"), 23) {
		t.Fatal("second upsert")
	}
	if _, e = c.PersistIncremental(v, l.Scan.Tree, 9); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || l.TotalSize != 900 || l.LastEventID != 9 || l.FileCount != 3 {
		t.Fatalf("cumulative load %+v %v", l, e)
	}
	if _, e = c.Save(v, l.Scan.Tree, 10); e != nil {
		t.Fatal(e)
	}
	if _, e = os.Stat(delta); !errors.Is(e, os.ErrNotExist) {
		t.Fatal("stale delta retained")
	}
	l, e = c.Load(v)
	if e != nil || l.DeltaApplied || l.TotalSize != 900 || l.LastEventID != 10 {
		t.Fatalf("compact %+v %v", l, e)
	}
}

func TestDiskIndexCacheEmptyDeltaAndWatermark(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 2); e != nil {
		t.Fatal(e)
	}
	l, _ := c.Load(v)
	r, e := c.PersistIncremental(v, l.Scan.Tree, 20)
	if e != nil || !r.Delta {
		t.Fatalf("empty delta %+v %v", r, e)
	}
	_, delta := c.Paths(v)
	d := diskCacheRead(t, delta)
	if len(d) != 128 || cacheU64(d, 60) != 0 || cacheU64(d, 76) != 0 {
		t.Fatal("metadata-only update not empty")
	}
	other, _ := NewDiskIndexCache(c.Root())
	if _, e = other.Save(v, tree, 10); !errors.Is(e, ErrDiskIndexStale) {
		t.Fatalf("older save accepted: %v", e)
	}
	// Simulate a new process: on-disk watermarks must also prevent regression.
	base, _ := c.Paths(v)
	diskIndexWrites.Lock()
	delete(diskIndexWrites.watermarks, base)
	diskIndexWrites.Unlock()
	if _, e = other.PersistIncremental(v, tree, 19); !errors.Is(e, ErrDiskIndexStale) {
		t.Fatalf("disk watermark ignored: %v", e)
	}
	l, e = c.Load(v)
	if e != nil || l.LastEventID != 20 {
		t.Fatal("watermark changed")
	}
}

func TestDiskIndexCacheRejectsBaseCorruption(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 7); e != nil {
		t.Fatal(e)
	}
	base, _ := c.Paths(v)
	good := diskCacheRead(t, base)
	cases := map[string]func([]byte) []byte{
		"unknown magic":             func(b []byte) []byte { b[0] = 'X'; return b },
		"unknown version":           func(b []byte) []byte { cachePut32(b, 8, 99); return b },
		"checksum":                  func(b []byte) []byte { b[128] ^= 0xff; return b },
		"header watermark checksum": func(b []byte) []byte { cachePut64(b, 56, 999); return b },
		"zero checksum":             func(b []byte) []byte { cachePut64(b, 72, 0); return b },
		"csr sentinel":              func(b []byte) []byte { cachePut32(b, int(cacheU32(b, 88))+int(cacheU32(b, 12))*4, 0); return b },
		"duplicate csr":             func(b []byte) []byte { off := int(cacheU32(b, 92)); copy(b[off+4:off+8], b[off:off+4]); return b },
		"forward parent":            func(b []byte) []byte { cachePut32(b, int(cacheU32(b, 84))+24, 0xc0000002); return b },
		"bad name":                  func(b []byte) []byte { cachePut32(b, int(cacheU32(b, 84))+28, math.MaxUint32); return b },
		"alignment":                 func(b []byte) []byte { cachePut32(b, 88, cacheU32(b, 88)+1); return b },
		"section overlap":           func(b []byte) []byte { cachePut32(b, 80, 128); return b },
		"huge count":                func(b []byte) []byte { cachePut32(b, 12, math.MaxUint32); return b },
		"stats":                     func(b []byte) []byte { cachePut64(b, 32, 999); return b },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			b := mutate(bytes.Clone(good))
			if name != "checksum" && name != "header watermark checksum" && name != "zero checksum" {
				cachePut64(b, 72, cacheChecksum(b, 72))
			}
			diskCacheWrite(t, base, b)
			if _, e := c.Load(v); !errors.Is(e, ErrDiskIndexCorrupt) {
				t.Fatalf("corrupt base accepted: %v", e)
			}
		})
	}
	for n := 0; n < len(good); n++ {
		diskCacheWrite(t, base, good[:n])
		if _, e := c.Load(v); e == nil {
			t.Fatalf("truncation %d accepted", n)
		}
	}
}

func TestDiskIndexCacheRejectedDeltaDoesNotMutateBase(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	l, _ := c.Load(v)
	modified := l.Scan.Tree.Clone()
	modified.UpsertFile(filepath.Join(v.Mount, "bad.bin"), 33)
	if _, e := c.PersistIncremental(v, modified, 5); e != nil {
		t.Fatal(e)
	}
	base, delta := c.Paths(v)
	baseBytes := diskCacheRead(t, base)
	good := diskCacheRead(t, delta)
	cases := map[string]func([]byte) []byte{
		"checksum":         func(b []byte) []byte { b[20] ^= 1; return b },
		"bad base binding": func(b []byte) []byte { cachePut64(b, 12, 999); return b },
		"self parent":      func(b []byte) []byte { cachePut32(b, 128, 0x40000000+cacheU32(baseBytes, 12)); return b },
		"bad override after append": func(b []byte) []byte {
			off := 128 + int(cacheU64(b, 60))*24 + int(cacheU64(b, 68))
			cachePut32(b, off, math.MaxUint32)
			return b
		},
		"missing overlay":      func(b []byte) []byte { cachePut64(b, 84, 0); return b },
		"watermark regression": func(b []byte) []byte { cachePut64(b, 20, 0); return b },
		"stats":                func(b []byte) []byte { cachePut64(b, 36, 999); return b },
		"huge count":           func(b []byte) []byte { cachePut64(b, 60, math.MaxUint64); return b },
		"trailing bytes":       func(b []byte) []byte { return append(b, 0) },
	}
	check := func(t *testing.T, b []byte) {
		t.Helper()
		diskCacheWrite(t, delta, b)
		restored, e := c.Load(v)
		if e != nil {
			t.Fatal(e)
		}
		if restored.DeltaApplied || restored.DeltaError == nil || restored.LastEventID != 1 || restored.TotalSize != 4196 || restored.Scan.Tree.nodeCount() != tree.nodeCount() {
			t.Fatalf("rejected delta leaked state: %+v", restored)
		}
		if !bytes.Equal(baseBytes, diskCacheRead(t, base)) {
			t.Fatal("load wrote base")
		}
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			b := mutate(bytes.Clone(good))
			if name != "checksum" {
				cachePut64(b, 100, cacheChecksum(b, 100))
			}
			check(t, b)
		})
	}
	for n := 0; n < len(good); n++ {
		check(t, good[:n])
	}
	diskCacheWrite(t, delta, good)
	restored, e := c.Load(v)
	if e != nil || !restored.DeltaApplied || restored.LastEventID != 5 {
		t.Fatal("valid delta stopped working")
	}
}

func TestDiskIndexCacheRejectsIncompleteAndFailedWrites(t *testing.T) {
	c, v, tree := diskCacheFixture(t)
	if _, e := c.Save(v, nil, 50); !errors.Is(e, ErrDiskIndexIncomplete) {
		t.Fatal(e)
	}
	base, _ := c.Paths(v)
	if _, e := os.Stat(base); !errors.Is(e, os.ErrNotExist) {
		t.Fatal("incomplete tree written")
	}
	// A failed high-watermark write must not poison a later successful save.
	if e := os.Mkdir(base, 0700); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Save(v, tree, 100); e == nil {
		t.Fatal("failed publication reported success")
	}
	if e := os.Remove(base); e != nil {
		t.Fatal(e)
	}
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	entries, e := os.ReadDir(c.Root())
	if e != nil {
		t.Fatal(e)
	}
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".tmp") {
			t.Fatal("temporary file leaked")
		}
	}
	tooWide := tree.Clone()
	slot := tooWide.slot(3)
	slot.Mtime = math.MaxUint64
	tooWide.setSlot(3, slot)
	if _, e := c.Save(v, tooWide, 2); !errors.Is(e, ErrDiskIndexCorrupt) {
		t.Fatalf("unrepresentable timestamp silently truncated: %v", e)
	}
}

func TestDiskIndexCacheOverrideAndConcurrentWatermarks(t *testing.T) {
	root := filepath.Join(t.TempDir(), "not-created")
	t.Setenv("QUICKCLEANER_CACHE_DIR", root)
	c, e := NewDiskIndexCache("")
	if e != nil || c.Root() != root {
		t.Fatal("override ignored", e)
	}
	if _, e = os.Stat(root); !errors.Is(e, os.ErrNotExist) {
		t.Fatal("constructor wrote cache")
	}
	_, v, tree := diskCacheFixture(t)
	var wg sync.WaitGroup
	for i := uint64(1); i <= 20; i++ {
		wg.Add(1)
		go func(event uint64) {
			defer wg.Done()
			_, e := c.Save(v, tree, event)
			if e != nil && !errors.Is(e, ErrDiskIndexStale) {
				t.Errorf("save %d: %v", event, e)
			}
		}(i)
	}
	wg.Wait()
	l, e := c.Load(v)
	if e != nil || l.LastEventID != 20 {
		t.Fatalf("async saves regressed: %+v %v", l, e)
	}
	t.Setenv("QUICKCLEANER_CACHE_DIR", "")
	if _, e = NewDiskIndexCache(""); e == nil {
		t.Fatal("empty override accepted")
	}
}

func TestDiskIndexCacheDefaultUsesTrustedUserDataDirectory(t *testing.T) {
	// Setenv registers restoration even if the caller originally had an override.
	t.Setenv("QUICKCLEANER_CACHE_DIR", "temporary")
	if err := os.Unsetenv("QUICKCLEANER_CACHE_DIR"); err != nil {
		t.Fatal(err)
	}
	data, ok := platform.UserDataDir()
	root, err := DiskIndexCacheRoot()
	if !ok || data == "" {
		if err == nil {
			t.Fatal("unknown user data directory accepted")
		}
		return
	}
	if err != nil || root != filepath.Join(data, "QuickCleaner") {
		t.Fatalf("not the trusted-user directory: %q %v", root, err)
	}
}

func TestDiskIndexCacheCompactsLargeDeltaAndForwardParents(t *testing.T) {
	c, v, _ := diskCacheFixture(t)
	tree := NewSizeTree(v, []TreeIndexEntry{{Name: "/", Used: true, IsDir: true}, {Parent: 2, Name: "child", Used: true, Size: 1}, {Parent: 0, Name: "parent", Used: true, IsDir: true}})
	if _, e := c.Save(v, tree, 1); e != nil {
		t.Fatal(e)
	}
	l, e := c.Load(v)
	if e != nil {
		t.Fatal(e)
	}
	if _, ok := l.Scan.Tree.FindNodeByPath(filepath.Join(v.Mount, "parent", "child")); !ok {
		t.Fatal("forward-parent acquisition lost")
	}
	for i := 0; i < DiskIndexDeltaCompactThreshold/2+1; i++ {
		l.Scan.Tree.entries = append(l.Scan.Tree.entries, treeSlot{TreeIndexEntry: TreeIndexEntry{Parent: 0, Name: "extra", Used: true, Size: 1}})
	}
	l.Scan.Tree.RebuildChildArrays()
	l.Scan.Tree.RecomputeAggregates()
	r, e := c.PersistIncremental(v, l.Scan.Tree, 2)
	if e != nil || !r.Compacted || r.Delta {
		t.Fatalf("large overlay not compacted %+v %v", r, e)
	}
	l, e = c.Load(v)
	if e != nil || l.LastEventID != 2 || l.FileCount != uint64(DiskIndexDeltaCompactThreshold/2+2) {
		t.Fatalf("large compact load %+v %v", l, e)
	}
}

// Fixtures constructed directly from the documented Rust ABIs, independent of
// the Go writers, including trailing names and a cumulative override/append.
func TestDiskIndexCacheReadsRustTrailingNamesLayout(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("POSIX mount-path fixture; the disk index cache is a macOS product path")
	}
	c, e := NewDiskIndexCache(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	v := VolumeFromMountPoint("/")
	b := make([]byte, 215)
	copy(b, "QCIDXV07")
	cachePut32(b, 8, 7)
	for o, val := range map[int]uint32{12: 2, 16: 7, 20: 1, 24: 1, 28: 1, 80: 208, 84: 144, 88: 192, 92: 204} {
		cachePut32(b, o, val)
	}
	cachePut64(b, 32, 1)
	cachePut64(b, 40, 1)
	cachePut64(b, 48, 42)
	cachePut64(b, 56, 17)
	copy(b[128:], "//")
	cachePut32(b, 144, 0xc0000000)
	cachePut64(b, 152, 42)
	cachePut32(b, 164, 1)
	cachePut32(b, 168, 0x40000000)
	cachePut32(b, 172, 3)
	cachePut64(b, 176, 42)
	cachePut32(b, 188, 1)
	// CSR starts are [0,1,1], then child_at=[1].
	cachePut32(b, 192, 0)
	cachePut32(b, 196, 1)
	cachePut32(b, 200, 1)
	cachePut32(b, 204, 1)
	copy(b[208:], []byte{1, 0, '/', 2, 0, 'a', 'b'})
	cachePut64(b, 72, cacheChecksum(b, 72))
	base, _ := c.Paths(v)
	diskCacheWrite(t, base, b)
	l, e := c.Load(v)
	if e != nil {
		t.Fatal(e)
	}
	if l.LastEventID != 17 || l.TotalSize != 42 {
		t.Fatal("ABI metadata")
	}
	if _, ok := l.Scan.Tree.FindNodeByPath("/ab"); !ok {
		t.Fatal("ABI path")
	}
	d := make([]byte, 200)
	copy(d, "QCDLTV01")
	cachePut32(d, 8, 1)
	for off, val := range map[int]uint64{12: cacheU64(b, 72), 20: 18, 36: 2, 44: 1, 52: 45, 60: 1, 68: 4, 76: 1, 84: 1, 92: 72} {
		cachePut64(d, off, val)
	}
	cachePut32(d, 128, 0x40000000)
	cachePut64(d, 136, 3)
	cachePut32(d, 144, 11)
	cachePut32(d, 148, 1)
	copy(d[152:], []byte{2, 0, 'c', 'd'})
	cachePut32(d, 156, 0)
	cachePut32(d, 160, 0xc0000000)
	cachePut64(d, 168, 45)
	cachePut32(d, 180, 2)
	cachePut32(d, 184, 0)
	cachePut32(d, 188, 2)
	cachePut32(d, 192, 1)
	cachePut32(d, 196, 2)
	cachePut64(d, 100, cacheChecksum(d, 100))
	_, delta := c.Paths(v)
	diskCacheWrite(t, delta, d)
	l, e = c.Load(v)
	if e != nil || !l.DeltaApplied || l.LastEventID != 18 || l.FileCount != 2 || l.TotalSize != 45 {
		t.Fatalf("delta ABI metadata: %+v %v", l, e)
	}
	if idx, ok := l.Scan.Tree.FindNodeByPath("/cd"); !ok || l.Scan.Tree.slot(idx).Mtime != 11 {
		t.Fatal("delta ABI append path/mtime")
	}
}

func TestDiskIndexCacheRootOnlyTrailingNames(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("POSIX mount-path fixture; the disk index cache is a macOS product path")
	}
	c, e := NewDiskIndexCache(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	v := VolumeFromMountPoint("/")
	b := make([]byte, 171)
	copy(b, "QCIDXV07")
	cachePut32(b, 8, 7)
	for off, val := range map[int]uint32{12: 1, 16: 3, 24: 1, 28: 1, 80: 168, 84: 136, 88: 160, 92: 168} {
		cachePut32(b, off, val)
	}
	cachePut64(b, 40, 1)
	copy(b[128:], "//")
	cachePut32(b, 136, 0xc0000000)
	copy(b[168:], []byte{1, 0, '/'})
	cachePut64(b, 72, cacheChecksum(b, 72))
	base, _ := c.Paths(v)
	diskCacheWrite(t, base, b)
	l, e := c.Load(v)
	if e != nil || l.DirCount != 1 || l.FileCount != 0 {
		t.Fatalf("empty CSR layout rejected: %+v %v", l, e)
	}
}

func TestDiskIndexCacheHealsAPFSMirror(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("APFS mirror fixture; macOS product path")
	}
	c, e := NewDiskIndexCache(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	v := VolumeFromMountPoint("/")
	tree := NewSizeTree(v, []TreeIndexEntry{
		{Name: "/", Used: true, IsDir: true},
		{Parent: 0, Name: "real.bin", Used: true, Size: 100},
		{Parent: 0, Name: "System", Used: true, IsDir: true},
		{Parent: 2, Name: "Volumes", Used: true, IsDir: true},
		{Parent: 3, Name: "Data", Used: true, IsDir: true},
		{Parent: 4, Name: "mirror.bin", Used: true, Size: 50},
	})
	if _, e = c.Save(v, tree, 7); e != nil {
		t.Fatal(e)
	}
	l, e := c.Load(v)
	if e != nil || l.TotalSize != 100 || l.FileCount != 1 || l.DirCount != 3 || l.LastEventID != 7 {
		t.Fatalf("mirror heal: %+v %v", l, e)
	}
	if _, ok := l.Scan.Tree.FindNodeByPath("/System/Volumes"); !ok {
		t.Fatal("mirror parent removed")
	}
	if _, e = c.PersistIncremental(v, l.Scan.Tree, 8); e != nil {
		t.Fatal(e)
	}
	l, e = c.Load(v)
	if e != nil || !l.DeltaApplied || l.TotalSize != 100 || l.LastEventID != 8 {
		t.Fatalf("healed state not persistent: %+v %v", l, e)
	}
}
