package app

import (
	"context"
	"fmt"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func uninstallApp(a core.InstalledApp) error { return core.RunMacAppUninstaller(a) }
func scanAppResiduals(ctx context.Context, a core.InstalledApp) (core.ResidualScanResult, error) {
	return core.ScanAppResiduals(ctx, a)
}
func appResidualsAllowed(ctx context.Context, a core.InstalledApp, _ []core.ResidualItem) bool {
	present, known := platform.BundleIsStillInstalled(ctx, a.RegistrySubpath)
	return known && !present && ctx.Err() == nil
}
func verifyResiduals(ctx context.Context, items []core.ResidualItem) ([]core.ResidualItem, error) {
	return core.VerifyAppResiduals(ctx, items)
}
func cleanResiduals(ctx context.Context, items []core.ResidualItem, p *core.CleanProgress) core.CleanReport {
	return core.CleanAppResiduals(ctx, items, p)
}
func scanOrphans(ctx context.Context) (*core.ResidualScanResult, error) {
	result, known := core.ScanOrphanResiduals(ctx)
	if !known {
		return nil, fmt.Errorf("orphan owner evidence unavailable; no cleanup authorized")
	}
	return result, nil
}
func orphanAbsent(ctx context.Context, items []core.ResidualItem) bool {
	return core.OrphanOwnersAllGone(ctx, items)
}
func newDiskOwner(ctx context.Context) (diskOwner, error) { return core.NewMacDiskIndex(ctx) }
func scanNativeVolume(ctx context.Context, v core.VolumeID) (*core.ScanResult, error) {
	s, err := core.ScanVolume(ctx, v, 0)
	return &s, err
}
