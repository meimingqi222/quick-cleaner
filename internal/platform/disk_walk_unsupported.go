//go:build !darwin

package platform

import "context"

func WalkMacVolume(ctx context.Context, volume VolumeID, progress func(MacWalkProgress)) (MacWalkResult, error) {
	if err := ctx.Err(); err != nil {
		return MacWalkResult{}, err
	}
	return MacWalkResult{}, ErrMacWalkUnsupported
}

func WalkMacVolumeFewThreads(ctx context.Context, volume VolumeID, progress func(MacWalkProgress)) (MacWalkResult, error) {
	return WalkMacVolume(ctx, volume, progress)
}
