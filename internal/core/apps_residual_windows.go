package core

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// ScanAppResiduals captures candidates before the official uninstaller removes
// the path evidence. It performs no deletion, registry writes or app launches.
func ScanAppResiduals(ctx context.Context, app InstalledApp) (ResidualScanResult, error) {
	r := platform.WindowsResidualReader{Context: ctx}
	data, shortcuts, local, system := r.Roots()
	return scanWindowsAppResiduals(ctx, app, r, windowsResidualRoots{data, shortcuts, local, system})
}

// VerifyAppResiduals filters the frozen candidate set after official uninstall.
// It never discovers additional targets or replaces scan-period identities.
func VerifyAppResiduals(ctx context.Context, items []ResidualItem) ([]ResidualItem, error) {
	r := platform.WindowsResidualReader{Context: ctx}
	_, _, _, system := r.Roots()
	return verifyWindowsAppResiduals(ctx, items, r, system)
}
