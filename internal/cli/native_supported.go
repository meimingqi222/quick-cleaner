//go:build windows || darwin

package cli

import (
	"context"
	"errors"
	"os"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func scanNative(ctx context.Context, volume core.VolumeID) (*core.SizeTree, error) {
	result, err := core.ScanVolume(ctx, volume, 20)
	if err != nil {
		return nil, err
	}
	if result.Spill != nil {
		defer result.Spill.Remove()
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		// Large macOS scans use the product's spill builder in a disposable
		// cache. The mapped tree remains readable after unlink on macOS.
		dir, err := os.MkdirTemp("", "quickcleaner-cli-index-")
		if err != nil {
			return nil, err
		}
		defer os.RemoveAll(dir)
		cache, err := core.NewDiskIndexCache(dir)
		if err != nil {
			return nil, err
		}
		built, err := core.BuildMacWalkSpillIndex(cache, volume, result.SpillRoot, result.Spill, result.SpillBlocked, 0)
		if err != nil {
			return nil, err
		}
		result = *built
	}
	if result.Tree == nil {
		return nil, errors.New("native scan returned no tree")
	}
	return result.Tree, nil
}
