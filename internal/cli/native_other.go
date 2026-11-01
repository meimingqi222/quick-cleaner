//go:build !windows && !darwin

package cli

import (
	"context"
	"errors"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func scanNative(context.Context, core.VolumeID) (*core.SizeTree, error) {
	return nil, errors.New("native volume analysis requires Windows or macOS; use --root for a live directory scan")
}
