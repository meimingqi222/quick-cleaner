package core

import (
	"context"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type nativeMacDiskIndexSource struct{}

func (nativeMacDiskIndexSource) Current() (uint64, error) { return platform.CurrentFSEventID() }
func (nativeMacDiskIndexSource) Changes(ctx context.Context, root string, since uint64, cache string) (*platform.FSEventChanges, error) {
	return platform.ReadFSEventChanges(ctx, root, since, cache)
}
func (nativeMacDiskIndexSource) Scan(ctx context.Context, volume VolumeID, few bool) (ScanResult, error) {
	started := time.Now()
	var raw platform.MacWalkResult
	var err error
	if few {
		raw, err = platform.WalkMacVolumeFewThreads(ctx, volume, nil)
	} else {
		raw, err = platform.WalkMacVolume(ctx, volume, nil)
	}
	return finishMacWalkScan(ctx, raw, err, started)
}

func NewMacDiskIndex(ctx context.Context) (*MacDiskIndex, error) {
	cache, err := NewDiskIndexCache("")
	if err != nil {
		return nil, err
	}
	return NewMacDiskIndexWithCache(ctx, cache), nil
}

// NewMacDiskIndexWithCache runs the product owner against an explicit cache,
// including disposable cache snapshots used by the command-line profiler.
func NewMacDiskIndexWithCache(ctx context.Context, cache *DiskIndexCache) *MacDiskIndex {
	return newMacDiskIndex(ctx, cache, nativeMacDiskIndexSource{})
}

// ScanVolume is an explicit fresh analysis, not a cache read. Product scan,
// search and disk controllers share a MacDiskIndex for cached/replayed analysis.
func ScanVolume(ctx context.Context, volume VolumeID, _ int) (ScanResult, error) {
	return (nativeMacDiskIndexSource{}).Scan(ctx, volume, false)
}
