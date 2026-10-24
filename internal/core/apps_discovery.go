package core

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func discoverPortableApps(ctx context.Context, registered []InstalledApp, candidates []platform.ShortcutAppEvidence, canonical func(string) (string, bool)) []InstalledApp {
	return discoverAppCandidates(ctx, registered, candidates, canonical, nil)
}

// launcherOwner identifies a registered app whose vendor launcher may live
// beside, not under, its install directory.
type launcherOwner struct{ name, dir string }

// Hiding only drops a duplicate list entry and grants no deletion: the shortcut
// name must equal the version-stripped display name and some target ancestor
// must share the install directory's name, so one edition never claims another.
func (o launcherOwner) claims(shortcut, target string) bool {
	if !strings.EqualFold(shortcut, o.name) {
		return false
	}
	for dir := filepath.Dir(target); filepath.Dir(dir) != dir; dir = filepath.Dir(dir) {
		if strings.EqualFold(filepath.Base(dir), o.dir) {
			return true
		}
	}
	return false
}

func discoverAppCandidates(ctx context.Context, registered []InstalledApp, candidates []platform.ShortcutAppEvidence, canonical func(string) (string, bool), inspect func(string) *InstalledApp) []InstalledApp {
	var covered []string
	var owners []launcherOwner
	for _, app := range registered {
		paths := []string{}
		if icon, ok := app.IconCacheKey(); ok {
			paths = append(paths, icon)
		}
		if location := DeduceInstallLocation(app); location != nil {
			paths = append(paths, *location)
			if app.Name != "" && !IsProtectedResidualPath(*location) {
				owners = append(owners, launcherOwner{name: windowsResidualShortName(app.Name), dir: filepath.Base(*location)})
			}
		}
		for _, path := range paths {
			if !IsProtectedResidualPath(path) {
				if path, ok := canonical(path); ok {
					covered = append(covered, Norm(path))
				}
			}
		}
	}
	byID := map[string]int{}
	var apps []InstalledApp
	for _, candidate := range candidates {
		if ctx.Err() != nil {
			return nil
		}
		target := candidate.Target
		if !filepath.IsAbs(target) || !strings.EqualFold(filepath.Ext(target), ".exe") || IsProtectedResidualPath(target) || IsSystemRootDir(filepath.Dir(target)) {
			continue
		}
		target, ok := canonical(target)
		if !ok && inspect != nil {
			target, ok = canonicalSourceTarget(candidate.Target, canonical)
		}
		if !ok || IsProtectedResidualPath(target) {
			continue
		}
		var source *InstalledApp
		if inspect != nil {
			source = inspect(target)
		}
		if info, err := os.Stat(target); (err != nil || !info.Mode().IsRegular()) && source == nil {
			continue
		}
		norm := Norm(target)
		if slices.ContainsFunc(covered, func(path string) bool { return atOrUnder(norm, path) }) {
			continue
		}
		shortcut := strings.TrimSuffix(filepath.Base(candidate.Shortcut), filepath.Ext(candidate.Shortcut))
		if source == nil && candidate.Shortcut != "" && slices.ContainsFunc(owners, func(o launcherOwner) bool { return o.claims(shortcut, target) }) {
			continue
		}
		id := "discovered:" + norm
		if source != nil {
			id = source.ID
		}
		index, exists := byID[id]
		if !exists {
			index = len(apps)
			byID[id] = index
			name := candidate.Shortcut
			if name == "" {
				name = target
			}
			name = strings.TrimSuffix(filepath.Base(name), filepath.Ext(name))
			location, icon := target, target
			app := InstalledApp{ID: id, Name: name, RegistryRoot: AppUnregistered, InstallLocation: &location, DisplayIcon: &icon, Discovery: &AppDiscovery{Executable: target, ProgramPaths: []string{target}}}
			if source != nil {
				app = *source
			}
			apps = append(apps, app)
		}
		if candidate.Shortcut != "" {
			apps[index].Discovery.Shortcuts = append(apps[index].Discovery.Shortcuts, candidate.Shortcut)
		}
	}
	for _, app := range apps {
		slices.Sort(app.Discovery.Shortcuts)
		app.Discovery.Shortcuts = slices.Compact(app.Discovery.Shortcuts)
		if plan := app.Discovery.Plan; plan != nil && plan.installation != nil {
			if err := plan.installation.ObserveScanPaths(app.Discovery.Shortcuts); err != nil {
				app.Discovery.Plan = nil
				app.Discovery.Uninstaller = nil
			}
		}
	}
	slices.SortFunc(apps, func(a, b InstalledApp) int { return strings.Compare(a.ID, b.ID) })
	return apps
}
