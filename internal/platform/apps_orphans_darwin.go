package platform

import (
	"context"
	"path/filepath"
	"strings"
	"time"
)

// Missing or mismatched present bundle identities invalidate the entire round.
// Historical registration paths alone prove neither installation nor removal.
func SupplementBundleOwners(ctx context.Context, owners []string, installed map[string]struct{}) (map[string]struct{}, bool) {
	out, ok := macInventoryCommand(ctx, 15*time.Second, "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister", "-dump")
	if !ok || !out.OK || strings.TrimSpace(string(out.Stderr)) != "" {
		return nil, false
	}
	records, ok := macRegistrationRecords(string(out.Stdout))
	if !ok {
		return nil, false
	}
	home, ok := UserHome()
	if !ok {
		return nil, false
	}
	return macSupplementOwners(ctx, owners, installed, filepath.Join(home, ".Trash"), func(id string) ([]string, bool) {
		paths, ok := macWorkspacePaths(ctx, id)
		if !ok {
			return nil, false
		}
		return append(paths, records[id]...), true
	}, func(path string) *string { return macPlistString(ctx, path, "CFBundleIdentifier", macInventoryCommand) })
}

func TrashedBundleIDs(ctx context.Context, home string) map[string]struct{} {
	return macTrashedBundleIDs(ctx, home, func(path string) *string { return macPlistString(ctx, path, "CFBundleIdentifier", macInventoryCommand) })
}
