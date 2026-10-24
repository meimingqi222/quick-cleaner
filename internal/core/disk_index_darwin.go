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
	if err != nil {
		return ScanResult{}, err
	}
	scan, err := BuildMacWalkScanResult(ctx, raw)
	if err == nil {
		scan.ElapsedMS = uint64(time.Since(started).Milliseconds())
	}
	return scan, err
}

func NewMacDiskIndex(ctx context.Context) (*MacDiskIndex, error) {
	cache, err := NewDiskIndexCache("")
	if err != nil {
		return nil, err
	}
	return newMacDiskIndex(ctx, cache, nativeMacDiskIndexSource{}), nil
}

// ScanVolume is an explicit fresh analysis, not a cache read. Product scan,
// search and disk controllers share a MacDiskIndex for cached/replayed analysis.
func ScanVolume(ctx context.Context, volume VolumeID, _ int) (ScanResult, error) {
	return (nativeMacDiskIndexSource{}).Scan(ctx, volume, false)
}
