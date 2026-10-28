package core

import (
	"context"
	"path/filepath"
	"slices"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func ListInstalledApps(ctx context.Context) ([]InstalledApp, error) {
	return ListInstalledAppsProgress(ctx, nil)
}

// ListInstalledAppsProgress reports the registry list before shortcut discovery
// and again before directory measurement. Measurement walks every install tree,
// so waiting for it leaves the apps page empty.
func ListInstalledAppsProgress(ctx context.Context, onList func([]InstalledApp)) ([]InstalledApp, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	records := platform.ReadWindowsAppRegistrations(ctx)
	apps := make([]InstalledApp, 0, len(records))
	for _, record := range records {
		app := appFromRecord(record)
		app.InstallDate, app.InstallDateRaw = ParseInstallDate(record.InstallDate)
		apps = append(apps, app)
	}
	report := func() {
		if onList != nil && ctx.Err() == nil && len(apps) > 0 {
			onList(slices.Clone(apps))
		}
	}
	report()
	candidates := platform.ReadWindowsAppShortcuts(ctx)
	snapshot, err := CurrentRules()
	if err != nil {
		return nil, err
	}
	if local, ok := platform.UserCacheDir(); ok {
		for _, definition := range snapshot.Bundle.Rules {
			if definition.App == nil {
				continue
			}
			for _, relative := range definition.App.Targets {
				target := filepath.Join(local, definition.App.DefaultHome, filepath.FromSlash(relative))
				candidates = append(candidates, platform.ShortcutAppEvidence{Target: target})
			}
		}
	}
	apps = append(apps, discoverAppCandidates(ctx, apps, candidates, platform.CanonicalAppPath, func(target string) *InstalledApp { return nativeInspectSourceApp(target, snapshot) })...)
	report()
	activity := platform.ReadWindowsAppActivity(ctx)
	shortcuts := platform.ReadWindowsShortcutActivity(ctx)
	return EnrichWindowsAppInventory(ctx, apps, activity, shortcuts), ctx.Err()
}
