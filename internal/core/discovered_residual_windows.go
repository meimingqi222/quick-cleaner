package core

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// ValidateDiscoveredResidualClean checks frozen discovered-app authority and live
// vacancy for exactly items. Success is not evidence that the whole app is absent.
// Portable discoveries own only captured files; registered apps use their ARP gate.
func ValidateDiscoveredResidualClean(ctx context.Context, app InstalledApp, items []ResidualItem) error {
	if d := app.Discovery; d != nil && d.Rule == nil && d.Plan == nil {
		return validatePortableResidualClean(ctx, app, items, platform.CanonicalAppPath, platform.ReadWindowsShortcutTarget, platform.ReadWindowsAppProcesses)
	}
	return validateDiscoveredResidualClean(ctx, app, items, nativeInspectSourceApp, platform.ReadWindowsAppProcesses)
}
