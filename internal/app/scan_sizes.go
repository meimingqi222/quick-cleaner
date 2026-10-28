package app

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type cachedDiskOwner interface {
	Cached(context.Context, core.VolumeID) (*core.ScanResult, error)
}

// Captured owner/trees are worker inputs; no UI-owned maps are read here.
func withCachedSizes(ctx context.Context, owner diskOwner, trees []*core.SizeTree) context.Context {
	if cached, ok := owner.(cachedDiskOwner); ok {
		if scan, err := cached.Cached(ctx, core.VolumeFromMountPoint("/")); err == nil && scan != nil && scan.Tree != nil {
			trees = append([]*core.SizeTree{scan.Tree}, trees...)
		}
	}
	return core.WithSizeIndexes(ctx, trees)
}

// Residual detection should not wait for a cold multi-million-entry index.
func withAvailableSizes(ctx context.Context, owner diskOwner, trees []*core.SizeTree) context.Context {
	if cached, ok := owner.(interface {
		Peek(core.VolumeID) *core.ScanResult
	}); ok {
		if scan := cached.Peek(core.VolumeFromMountPoint("/")); scan != nil && scan.Tree != nil {
			trees = append([]*core.SizeTree{scan.Tree}, trees...)
		}
	}
	return core.WithSizeIndexes(ctx, trees)
}
