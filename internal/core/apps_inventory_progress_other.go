//go:build !windows

package core

import "context"

func ListInstalledAppsProgress(ctx context.Context, onList func([]InstalledApp)) ([]InstalledApp, error) {
	apps, err := ListInstalledApps(ctx)
	if err != nil || onList == nil || len(apps) == 0 {
		return apps, err
	}
	onList(apps)
	return apps, nil
}
