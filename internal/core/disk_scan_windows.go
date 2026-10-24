package core

import (
	"context"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// ScanVolume never requests elevation or substitutes a partial directory walk
// for an unreadable raw NTFS volume.
func ScanVolume(ctx context.Context, volume VolumeID, topN int) (ScanResult, error) {
	started := time.Now()
	raw, err := platform.ScanMFT(ctx, volume)
	if err != nil {
		return ScanResult{}, err
	}
	result, err := BuildMFTScanResult(ctx, raw, topN)
	if err != nil {
		return ScanResult{}, err
	}
	result.ElapsedMS = uint64(time.Since(started).Milliseconds())
	return result, nil
}
