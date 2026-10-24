package app

import (
	"context"
	"fmt"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func uninstallApp(a core.InstalledApp) error {
	if a.Discovery != nil {
		return core.RunDiscoveredAppUninstallerReported(a).Result
	}
	return core.RunRegisteredAppUninstallerReported(a).Result
}
func scanAppResiduals(ctx context.Context, a core.InstalledApp) (core.ResidualScanResult, error) {
	return core.ScanAppResiduals(ctx, a)
}
func appResidualsAllowed(ctx context.Context, a core.InstalledApp, items []core.ResidualItem) bool {
	if ctx.Err() != nil {
		return false
	}
	if a.Discovery != nil {
		return core.ValidateDiscoveredResidualClean(ctx, a, items) == nil
	}
	if a.RegistrySubpath == "" {
		return false
	}
	if a.RegistryRoot != core.AppHKLM && a.RegistryRoot != core.AppHKLM32 && a.RegistryRoot != core.AppHKCU {
		return false
	}
	return !platform.AppRegistrationPresent(a.RegistryRoot.Label(), a.RegistrySubpath)
}
func verifyResiduals(ctx context.Context, items []core.ResidualItem) ([]core.ResidualItem, error) {
	return core.VerifyAppResiduals(ctx, items)
}
func cleanResiduals(ctx context.Context, items []core.ResidualItem, p *core.CleanProgress) core.CleanReport {
	return core.CleanAppResiduals(ctx, items, p)
}
func scanOrphans(context.Context) (*core.ResidualScanResult, error) {
	return nil, fmt.Errorf("orphan residual inventory is implemented for macOS only")
}
func orphanAbsent(context.Context, []core.ResidualItem) bool { return false }
func newDiskOwner(context.Context) (diskOwner, error)        { return nil, nil }
func scanNativeVolume(ctx context.Context, v core.VolumeID) (*core.ScanResult, error) {
	s, err := core.ScanVolume(ctx, v, 200)
	return &s, err
}
