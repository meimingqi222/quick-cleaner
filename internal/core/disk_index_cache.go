package core

import (
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

var (
	ErrDiskIndexCorrupt    = errors.New("invalid disk index")
	ErrDiskIndexIncomplete = errors.New("incomplete disk index cannot be persisted")
	ErrDiskIndexStale      = errors.New("disk index event watermark is stale")
)

const DiskIndexDeltaCompactThreshold = 200_000

// DiskIndexCache owns paths, not trees. Callers must serialize tree mutation
// with Load/Save/PersistIncremental. Publication is serialized across instances
// in this process; multiple writer processes are not supported.
type DiskIndexCache struct{ root string }

// DiskIndexCacheRoot is also the path FSEvents must exclude from observation.
// It does not create directories. The environment override includes empty values
// only as an error, never as permission to write to the working directory.
func DiskIndexCacheRoot() (string, error) {
	if root, ok := os.LookupEnv("QUICKCLEANER_CACHE_DIR"); ok {
		if root == "" {
			return "", errors.New("empty QUICKCLEANER_CACHE_DIR")
		}
		return filepath.Abs(root)
	}
	data, ok := platform.UserDataDir()
	if !ok || data == "" {
		return "", errors.New("trusted user data directory unavailable")
	}
	return filepath.Join(data, "QuickCleaner"), nil
}

// NewDiskIndexCache uses the explicit root, or DiskIndexCacheRoot if empty.
// Construction and reads never create or write the cache directory.
func NewDiskIndexCache(root string) (*DiskIndexCache, error) {
	var err error
	if root == "" {
		root, err = DiskIndexCacheRoot()
	} else {
		root, err = filepath.Abs(root)
	}
	if err != nil {
		return nil, err
	}
	return &DiskIndexCache{root: root}, nil
}

func (c *DiskIndexCache) Root() string { return c.root }
func (c *DiskIndexCache) Paths(volume VolumeID) (base, delta string) {
	stem := "scan-index-" + hex.EncodeToString([]byte(volume.Mount))
	return filepath.Join(c.root, stem+".bin"), filepath.Join(c.root, stem+".delta.bin")
}

type DiskIndexMetadata struct {
	LastEventID, ScannedAt, FileCount, DirCount, TotalSize uint64
}

type LoadedDiskIndex struct {
	Scan ScanResult
	DiskIndexMetadata
	BaseChecksum uint64
	DeltaApplied bool
	// A bad delta never partially modifies Scan. The usable base watermark is
	// returned instead, so the caller can replay events from that older point.
	DeltaError error
}

type DiskIndexSaveResult struct {
	DiskIndexMetadata
	BaseChecksum uint64
	Delta        bool
	Compacted    bool
}

var diskIndexWrites = struct {
	sync.Mutex
	watermarks map[string]uint64
}{}

func init() {
	diskIndexWrites.watermarks = make(map[string]uint64)
}

func (c *DiskIndexCache) Load(volume VolumeID) (*LoadedDiskIndex, error) {
	diskIndexWrites.Lock()
	defer diskIndexWrites.Unlock()
	return c.load(volume)
}

func (c *DiskIndexCache) load(volume VolumeID) (*LoadedDiskIndex, error) {
	base, delta := c.Paths(volume)
	mapped, mapErr := mapDiskIndex(base)
	defer runtime.KeepAlive(mapped)
	var b []byte
	var err error
	if mapErr == nil {
		b = mapped.bytes()
	} else {
		b, err = readDiskIndexRegular(base)
		if err != nil {
			return nil, err
		}
	}
	decoded, err := decodeDiskIndexWithMapping(b, volume, mapped)
	if err != nil {
		if mapped != nil {
			mapped.close()
		}
		return nil, err
	}
	if mapped != nil {
		mapped.releasePages()
	}
	if err := restoreDiskIndexCoverage(base, b, decoded.tree, decoded.checksum, decoded.meta.LastEventID, false); err != nil {
		if mapErr == nil {
			mapped.close()
		}
		return nil, err
	}
	tree, meta := decoded.tree, decoded.meta
	out := &LoadedDiskIndex{BaseChecksum: decoded.checksum}
	if d, e := readDiskIndexRegular(delta); e == nil {
		candidate, dm, e := decodeDiskIndexDelta(d, decoded)
		if e == nil {
			e = restoreDiskIndexCoverage(delta, d, candidate, decoded.checksum, dm.LastEventID, true)
		}
		if e == nil {
			tree, meta, out.DeltaApplied = candidate, dm, true
		} else {
			out.DeltaError = e
		}
	} else if !errors.Is(e, os.ErrNotExist) {
		out.DeltaError = e
	}
	if meta.TotalSize == 0 {
		meta.TotalSize = tree.SizeOf(0)
	}
	// Heal historical APFS firmlink duplicate indexes without treating absent
	// nodes as authoritative evidence of filesystem absence.
	if volume.Mount == "/" {
		if idx, ok := tree.FindNodeByPath("/System/Volumes/Data"); ok {
			tree.RemoveSubtreeInplace(idx)
			meta.FileCount, meta.DirCount, meta.TotalSize = tree.CountUsedFiles(), tree.CountUsedDirs(), tree.SizeOf(0)
		}
	}
	out.DiskIndexMetadata = meta
	n := meta.FileCount + meta.DirCount
	out.Scan = ScanResult{Volume: volume, Tree: tree, TotalSize: meta.TotalSize, FileCount: meta.FileCount, DirCount: meta.DirCount, RecordsRead: n, RecordsExpected: n, UniqueSize: meta.TotalSize, UniqueFiles: meta.FileCount}
	return out, nil
}

// Save atomically publishes a compact Rust-v7 base and invalidates its delta.
// Unknown coverage is preserved in a checksum-bound required sidecar.
func (c *DiskIndexCache) Save(volume VolumeID, tree *SizeTree, lastEventID uint64) (DiskIndexSaveResult, error) {
	return c.persist(volume, tree, lastEventID, false)
}

// PersistIncremental replaces the cumulative Rust-v1 overlay when stable slots
// still match the base, including metadata-only updates. Otherwise it compacts.
// A tree loaded from a mapped base keeps that base; its edits are overrides, so
// the diff reads the mapping instead of a second heap copy of every record.
func (c *DiskIndexCache) PersistIncremental(volume VolumeID, tree *SizeTree, lastEventID uint64) (DiskIndexSaveResult, error) {
	return c.persist(volume, tree, lastEventID, true)
}

func (c *DiskIndexCache) persist(volume VolumeID, tree *SizeTree, event uint64, incremental bool) (DiskIndexSaveResult, error) {
	diskIndexWrites.Lock()
	defer diskIndexWrites.Unlock()
	var result DiskIndexSaveResult
	if tree == nil {
		return result, ErrDiskIndexIncomplete
	}
	if tree.volume.Mount != volume.Mount {
		return result, fmt.Errorf("%w: volume mismatch", ErrDiskIndexCorrupt)
	}
	if _, _, err := canonicalDiskIndexCoverage(volume.Mount, tree.blocked); err != nil {
		return result, err
	}
	base, delta := c.Paths(volume)
	if event < diskIndexWrites.watermarks[base] {
		return result, ErrDiskIndexStale
	}
	// Before the whole-tree validation below: skipping it is the point.
	if incremental {
		if r, ok, err := c.persistOverrides(volume, tree, event, base, delta); ok {
			return r, err
		}
	}
	if err := validateDiskTree(tree, false); err != nil {
		return result, err
	}
	previous, loadErr := c.load(volume)
	if loadErr == nil && event < previous.LastEventID {
		return result, ErrDiskIndexStale
	}
	meta := DiskIndexMetadata{LastEventID: event, ScannedAt: uint64(time.Now().Unix()), FileCount: tree.CountUsedFiles(), DirCount: tree.CountUsedDirs(), TotalSize: tree.SizeOf(0)}
	result.DiskIndexMetadata = meta
	// A missing-required base sidecar cannot be repaired by adding a delta:
	// loading must still be able to fall back safely to that base. Rewrite it.
	if incremental && loadErr == nil {
		if bytes, err := readDiskIndexRegular(base); err == nil {
			if original, err := decodeDiskIndex(bytes, volume); err == nil {
				if d, count, err := encodeDiskIndexDelta(tree, original, meta); err == nil && count <= DiskIndexDeltaCompactThreshold {
					if err := publishDiskIndexCovered(delta, d, tree, original.checksum, event, true); err == nil {
						diskIndexWrites.watermarks[base] = event
						result.BaseChecksum, result.Delta = original.checksum, true
						return result, nil
					}
				}
			}
		}
	}
	return c.publishBase(volume, tree, meta, incremental)
}

// The caller holds diskIndexWrites and has validated the snapshot and event
// watermark, either as a full tree or a validated base plus checked edits.
func (c *DiskIndexCache) publishBase(volume VolumeID, tree *SizeTree, meta DiskIndexMetadata, compacted bool) (DiskIndexSaveResult, error) {
	base, delta := c.Paths(volume)
	result := DiskIndexSaveResult{DiskIndexMetadata: meta}
	b, err := encodeDiskIndex(tree, volume, meta)
	if err != nil {
		return DiskIndexSaveResult{}, err
	}
	if err := publishDiskIndexCovered(base, b, tree, 0, meta.LastEventID, false); err != nil {
		return DiskIndexSaveResult{}, err
	}
	diskIndexWrites.watermarks[base] = meta.LastEventID
	result.BaseChecksum, result.Compacted = cacheU64(b, 72), compacted
	// A stale delta is checksum-bound to the previous base and cannot apply even
	// if removal fails. Report the cleanup error; the base is already published.
	if err := os.Remove(delta); err != nil && !errors.Is(err, os.ErrNotExist) {
		return result, err
	}
	return result, nil
}

func publishDiskIndex(path string, b []byte) error {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(path), ".scan-index-*.tmp")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if _, err = f.Write(b); err == nil {
		err = f.Sync()
	}
	closeErr := f.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	if err = os.Rename(f.Name(), path); err != nil {
		return err
	}
	return syncDiskIndexDirectory(path)
}

func syncDiskIndexDirectory(path string) error {
	// Sync the directory on Unix so publication survives a power loss too.
	// Windows cannot flush a directory handle opened by os.Open (Access is
	// denied); its rename is already the publication point.
	if runtime.GOOS == "windows" {
		return nil
	}
	dir, err := os.Open(filepath.Dir(path))
	if err != nil {
		return err
	}
	defer dir.Close()
	return dir.Sync()
}
