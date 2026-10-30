//go:build !darwin

package main

import (
	"context"
	"errors"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func newNativeOwner(context.Context, *core.DiskIndexCache) (indexOwner, error) {
	return nil, errors.New("refresh mode requires macOS; use -mode cache for saved indexes")
}
