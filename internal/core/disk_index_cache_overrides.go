package core

import (
	"errors"
	"io"
	"math"
	"os"
	"slices"
	"time"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// persistOverrides publishes a delta straight from a mapped tree's overrides
// and appended nodes, as Rust's override-only mutation does. The mapped base
// was fully validated at load and is read-only, so nothing else can differ.
// The full path loads and decodes the base twice and validates every node:
// seconds and gigabytes per refresh on a multi-million-node index.
// ok is false whenever a precondition is not known to hold; the caller then
// takes the full path, which makes the final decision.
func (c *DiskIndexCache) persistOverrides(volume VolumeID, tree *SizeTree, event uint64, basePath, deltaPath string) (DiskIndexSaveResult, bool, error) {
	var result DiskIndexSaveResult
	if tree.base == nil || tree.baseCount == 0 {
		return result, false, nil
	}
	header := tree.base.header()
	checksum, baseEvent := cacheU64(header, 72), cacheU64(header, 56)
	onDisk, err := readDiskIndexPrefix(basePath, 128)
	if err != nil || cacheU64(onDisk, 72) != checksum {
		return result, false, nil
	}
	// A base whose required coverage is gone must be rewritten, not patched.
	if err := restoreDiskIndexCoverage(basePath, header, &SizeTree{volume: volume}, checksum, baseEvent, false); err != nil {
		return result, false, nil
	}
	previous := baseEvent
	if d, err := readDiskIndexPrefix(deltaPath, 128); err == nil {
		if string(d[:8]) != "QCDLTV01" || cacheU64(d, 12) != checksum {
			return result, false, nil
		}
		previous = max(previous, cacheU64(d, 20))
	} else if !errors.Is(err, os.ErrNotExist) {
		return result, false, nil
	}
	if event < baseEvent {
		return result, true, ErrDiskIndexStale
	}
	if event < previous {
		// Only a valid delta makes this stale; the full path decodes it.
		return result, false, nil
	}
	overrides, files, dirs, ok := tree.validateOverrides(cacheU64(header, 32), cacheU64(header, 40))
	if !ok {
		return result, false, nil
	}
	extra := map[uint32][]uint32{}
	for i := range tree.entries {
		if e := &tree.entries[i]; e.Used {
			extra[e.Parent] = nil
		}
	}
	for p := range extra {
		var kids []uint32
		for _, k := range tree.ChildIndices(p) {
			if k > 0 && tree.Valid(k) && tree.slot(k).Parent == p {
				kids = append(kids, k)
			}
		}
		slices.Sort(kids)
		extra[p] = slices.Compact(kids)
	}
	meta := DiskIndexMetadata{LastEventID: event, ScannedAt: uint64(time.Now().Unix()), FileCount: files, DirCount: dirs, TotalSize: tree.SizeOf(0)}
	d, count, err := writeDiskIndexDelta(tree, tree.baseCount, checksum, meta, overrides, func(idx int) uint32 { return tree.base.nameOff(uint32(idx)) }, extra)
	if err != nil || count > DiskIndexDeltaCompactThreshold {
		return result, false, nil
	}
	if err := publishDiskIndexCovered(deltaPath, d, tree, checksum, event, true); err != nil {
		return result, false, nil
	}
	diskIndexWrites.watermarks[basePath] = event
	result.DiskIndexMetadata = meta
	result.BaseChecksum, result.Delta = checksum, true
	return result, true, nil
}

// validateOverrides checks only what can differ from the validated base and
// returns the ascending changed base indexes with current used file/dir
// counts. Base identity (parent and name) must be unchanged; appended nodes
// must point backwards, so no cycle can form.
func (t *SizeTree) validateOverrides(baseFiles, baseDirs uint64) ([]int, uint64, uint64, bool) {
	if t.nodeCount() == 0 || uint64(t.nodeCount()) > uint64(cacheParentMask)+1 || !t.IsDir(0) || t.slot(0).Parent != 0 {
		return nil, 0, 0, false
	}
	files, dirs := int64(baseFiles), int64(baseDirs)
	count := func(e treeSlot, sign int64) {
		if !e.Used {
			return
		}
		if e.IsDir {
			dirs += sign
		} else {
			files += sign
		}
	}
	valid := func(i uint32, e treeSlot) bool {
		if e.Parent > cacheParentMask || e.Mtime > math.MaxUint32 || e.files > math.MaxUint32 || len(e.Name) > math.MaxUint16 || !utf8.ValidString(e.Name) {
			return false
		}
		if !e.Used {
			return true
		}
		if i > 0 && !validDiskIndexName(e.Name) {
			return false
		}
		return i == 0 || (t.IsDir(e.Parent) && e.Parent != i)
	}
	changed := make([]int, 0, len(t.overrides))
	for idx, e := range t.overrides {
		b := t.base.slot(idx)
		if e.Parent != b.Parent || e.Name != b.Name || !valid(idx, e) {
			return nil, 0, 0, false
		}
		if e == b {
			continue
		}
		// A base directory that stops being one may not keep live children.
		if b.IsDir && (!e.Used || !e.IsDir) {
			for _, k := range t.ChildIndices(idx) {
				if t.Valid(k) && t.slot(k).Parent == idx {
					return nil, 0, 0, false
				}
			}
		}
		count(b, -1)
		count(e, 1)
		changed = append(changed, int(idx))
	}
	for i, e := range t.entries {
		idx := uint32(t.baseCount + i)
		if !valid(idx, e) || (e.Used && e.Parent >= idx) {
			return nil, 0, 0, false
		}
		count(e, 1)
	}
	if files < 0 || dirs < 0 {
		return nil, 0, 0, false
	}
	slices.Sort(changed)
	return changed, uint64(files), uint64(dirs), true
}

// readDiskIndexPrefix reads n bytes through the same no-link checks as
// readDiskIndexRegular without reading the rest of the file.
func readDiskIndexPrefix(path string, n int) ([]byte, error) {
	before, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || platform.DevEnvStorageIsReparse(before) {
		return nil, cacheInvalid("cache path is not a regular file")
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	after, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if !os.SameFile(before, after) {
		return nil, cacheInvalid("cache file changed while opening")
	}
	b := make([]byte, n)
	if _, err := io.ReadFull(f, b); err != nil {
		return nil, err
	}
	return b, nil
}
