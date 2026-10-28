package platform

import (
	"context"
	"os"
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

// TrashedApp is a .app bundle still sitting in ~/.Trash: the bundle keeps its
// Info.plist there, so the trash is also the record of what was uninstalled.
type TrashedApp struct {
	ID, Name, Path string
}

// TrashedApps lists identifiable .app bundles currently in the trash.
func TrashedApps(ctx context.Context, home string) []TrashedApp {
	trash := filepath.Join(home, ".Trash")
	entries, err := os.ReadDir(trash)
	if err != nil {
		return nil
	}
	var apps []TrashedApp
	for _, entry := range entries {
		if ctx.Err() != nil {
			return nil
		}
		if !entry.IsDir() || filepath.Ext(entry.Name()) != ".app" {
			continue
		}
		path := filepath.Join(trash, entry.Name())
		plist := filepath.Join(path, "Contents", "Info.plist")
		id := macPlistString(ctx, plist, "CFBundleIdentifier", macInventoryCommand)
		if id == nil || !ValidBundleID(*id) {
			continue
		}
		name := macPlistString(ctx, plist, "CFBundleDisplayName", macInventoryCommand)
		if name == nil {
			name = macPlistString(ctx, plist, "CFBundleName", macInventoryCommand)
		}
		if name == nil || *name == "" {
			base := strings.TrimSuffix(entry.Name(), ".app")
			name = &base
		}
		apps = append(apps, TrashedApp{ID: strings.ToLower(*id), Name: *name, Path: path})
	}
	return apps
}

// InstalledAppNames enumerates first-level .app bundles under the standard
// roots and maps lowercase bundle ID to display name. It feeds the uninstall
// watcher's baseline: an ID that disappears between rounds was deleted in
// place, which the trash never sees.
func InstalledAppNames(ctx context.Context, home string) map[string]TrashedApp {
	return InstalledAppNamesCached(ctx, home, nil)
}

// AppSnapshotCache lets the uninstall watcher diff app roots every round
// without re-parsing every Info.plist: a bundle whose modification time did
// not change keeps its recorded identity. Callers own the map and keep it on
// one goroutine. Pass nil for a one-off snapshot.
type AppSnapshotCache map[string]appSnapshotEntry

type appSnapshotEntry struct {
	id, name string
	modTime  time.Time
}

func InstalledAppNamesCached(ctx context.Context, home string, cache AppSnapshotCache) map[string]TrashedApp {
	out := map[string]TrashedApp{}
	for _, root := range macAppRoots(home) {
		entries, err := os.ReadDir(root.path)
		if err != nil {
			continue
		}
		for _, entry := range entries {
			if ctx.Err() != nil {
				return out
			}
			path := filepath.Join(root.path, entry.Name())
			if !macRealBundle(path) {
				continue
			}
			info, statErr := entry.Info()
			if statErr == nil && cache != nil {
				if cached, ok := cache[path]; ok && info.ModTime() == cached.modTime {
					out[cached.id] = TrashedApp{ID: cached.id, Name: cached.name, Path: path}
					continue
				}
			}
			plist := filepath.Join(path, "Contents", "Info.plist")
			id := macPlistString(ctx, plist, "CFBundleIdentifier", macInventoryCommand)
			if id == nil || !ValidBundleID(*id) {
				continue
			}
			name := macPlistString(ctx, plist, "CFBundleDisplayName", macInventoryCommand)
			if name == nil {
				name = macPlistString(ctx, plist, "CFBundleName", macInventoryCommand)
			}
			if name == nil || *name == "" {
				base := strings.TrimSuffix(entry.Name(), ".app")
				name = &base
			}
			lower := strings.ToLower(*id)
			out[lower] = TrashedApp{ID: lower, Name: *name, Path: path}
			if cache != nil && statErr == nil {
				cache[path] = appSnapshotEntry{id: lower, name: *name, modTime: info.ModTime()}
			}
		}
	}
	return out
}
