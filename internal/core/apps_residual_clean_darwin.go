package core

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"slices"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// CleanAppResiduals accepts explicitly selected, post-uninstall candidates. The
// caller must confirm application absence (or OrphanOwnersAllGone for orphans).
func CleanAppResiduals(ctx context.Context, items []ResidualItem, progress *CleanProgress) CleanReport {
	var paths []string
	for _, item := range items {
		if item.Kind.Type == ResidualFile || item.Kind.Type == ResidualDirectory {
			paths = append(paths, item.Kind.Path)
		}
	}
	return cleanMacResidualsWith(ctx, items, progress, SpotCheckInUse(paths), macResidualCleaner{
		needsPrivilege: platform.NeedsResidualElevation,
		elevated:       platform.RemovePrivilegedResiduals,
		bootout: func(ctx context.Context, path string) {
			platform.RunCommandTreeContext(ctx, exec.Command("/bin/launchctl", "bootout", fmt.Sprintf("gui/%d", os.Getuid()), path), 3*time.Second)
		},
		deactivate: deactivateMacSystemExtension,
		dispose:    Dispose,
	})
}

func deactivateMacSystemExtension(ctx context.Context, extension ResidualKind) CleanResult {
	run, ok := platform.RunCommandTreeContext(ctx, exec.Command("/usr/bin/csrutil", "status"), 3*time.Second)
	if !ok || strings.Contains(string(run.Stdout), "enabled") {
		return CleanManualAction
	}
	platform.RunCommandTreeContext(ctx, exec.Command("/usr/bin/systemextensionsctl", "uninstall", extension.TeamID, extension.BundleID), 30*time.Second)
	listed, known := listMacSystemExtensions(ctx)
	if !known || slices.ContainsFunc(listed, func(active ResidualKind) bool { return asciiLower(active.BundleID) == asciiLower(extension.BundleID) }) {
		return CleanFailed
	}
	return CleanOK
}
