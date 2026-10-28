package core

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// CleanAppResiduals consumes explicitly selected, post-uninstall residuals. The
// caller must establish application absence before invoking this destructive API.
func CleanAppResiduals(ctx context.Context, items []ResidualItem, progress *CleanProgress) CleanReport {
	r := platform.WindowsResidualCleanAdapter{Context: ctx}
	return cleanWindowsResidualsWith(ctx, items, progress, windowsResidualCleaner{
		pending:     r.PendingRenameWords,
		terminate:   func(paths []string, allowed func(string) bool) { _ = r.TerminateUnder(paths, allowed) },
		stopService: func(name string) { _ = r.StopService(name) },
		absent: func(kind ResidualKind) (bool, bool) {
			if kind.Type == ResidualScheduledTask {
				return r.TaskAbsent(kind.Path)
			}
			return r.RegistryAbsent(kind.Root.Label(), kind.Path, kind.Name, kind.Type == ResidualRegistryValue)
		},
		remove: func(kind ResidualKind) error {
			if kind.Type == ResidualScheduledTask {
				return r.DeleteTask(kind.Path)
			}
			return r.DeleteRegistry(kind.Root.Label(), kind.Path, kind.Name, kind.Type == ResidualRegistryValue)
		},
		dispose: func(path string, disposal Disposal, p *CleanProgress) CleanResult {
			return recyclePathWith(path, p, r.MoveToTrash)
		},
		wait: windowsResidualWait,
	})
}

// Windows keeps occupancy evidence empty, matching the original Windows API.
// Cleanup relies on native sharing errors rather than a fabricated vacancy scan.
func DetectAppResidualOccupancy(_ context.Context, _ InstalledApp) ResidualOccupancy {
	return ResidualOccupancy{}
}
