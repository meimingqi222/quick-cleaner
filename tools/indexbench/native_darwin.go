package main

import (
	"context"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func newNativeOwner(ctx context.Context, cache *core.DiskIndexCache) (indexOwner, error) {
	return core.NewMacDiskIndexWithCache(ctx, cache), nil
}
