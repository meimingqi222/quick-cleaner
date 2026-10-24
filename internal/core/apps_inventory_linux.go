package core

import (
	"context"
	"errors"
)

func ListInstalledApps(ctx context.Context) ([]InstalledApp, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return nil, errors.New("application inventory is supported on Windows and macOS only")
}
