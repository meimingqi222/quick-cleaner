//go:build !windows && !darwin

package app

import (
	"context"
	"errors"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func uninstallApp(core.InstalledApp) error {
	return errors.New("application removal is supported on Windows and macOS only")
}
func scanAppResiduals(context.Context, core.InstalledApp) (core.ResidualScanResult, error) {
	return core.ResidualScanResult{}, errors.New("application residual inventory is supported on Windows and macOS only")
}
func appResidualsAllowed(context.Context, core.InstalledApp, []core.ResidualItem) bool {
	return false
}
func verifyResiduals(context.Context, []core.ResidualItem) ([]core.ResidualItem, error) {
	return nil, errors.New("native residual verification unavailable")
}
func cleanResiduals(_ context.Context, items []core.ResidualItem, _ *core.CleanProgress) core.CleanReport {
	var report core.CleanReport
	for _, item := range items {
		report.Record(item.Kind.DisplayLabel(), core.CleanFailed)
	}
	return report
}
func scanOrphans(context.Context) (*core.ResidualScanResult, error) {
	return nil, errors.New("orphan residual inventory is implemented for macOS only")
}
func orphanAbsent(context.Context, []core.ResidualItem) bool { return false }
func newDiskOwner(context.Context) (diskOwner, error)        { return nil, nil }
func scanNativeVolume(context.Context, core.VolumeID) (*core.ScanResult, error) {
	return nil, errors.New("native volume indexing is supported on Windows and macOS only")
}
