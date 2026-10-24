package core

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func ListInstalledApps(ctx context.Context) ([]InstalledApp, error) {
	if _, err := CurrentRules(); err != nil {
		return nil, err
	}
	records := platform.ReadMacAppRecords(ctx)
	apps := make([]InstalledApp, 0, len(records))
	for _, record := range records {
		if ctx.Err() != nil {
			break
		}
		app := appFromRecord(record)
		if app.InstallLocation != nil {
			app.UninstallString = MacAppUninstaller(*app.InstallLocation, app.RegistrySubpath)
			app.UninstallerMissing = app.UninstallString == nil
		}
		apps = append(apps, app)
	}
	return apps, ctx.Err()
}
