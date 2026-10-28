//go:build !windows

package platform

import "context"

func ScanMFT(ctx context.Context, volume VolumeID) (MFTScanResult, error) {
	if err := ctx.Err(); err != nil {
		return MFTScanResult{}, err
	}
	return MFTScanResult{}, ErrMFTUnsupported
}
