package core

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
	"golang.org/x/text/cases"
	"golang.org/x/text/language"
)

func appFromRecord(record platform.AppRecord) InstalledApp {
	root := AppUnregistered
	switch record.RegistryRoot {
	case "HKLM":
		root = AppHKLM
	case "HKLM32":
		root = AppHKLM32
	case "HKCU":
		root = AppHKCU
	case "SystemApp":
		root = AppSystem
	}
	return InstalledApp{
		ID: record.ID, Name: record.Name, Version: record.Version, Publisher: record.Publisher,
		LastUsedDate: record.LastUsedDate, InstallDate: record.InstallDate,
		LastUsedRaw: record.LastUsedRaw, InstallDateRaw: record.InstallDateRaw,
		InstallLocation: record.InstallLocation, DisplayIcon: record.DisplayIcon,
		UninstallString: record.UninstallString, QuietUninstallString: record.QuietUninstallString,
		EstimatedSize: record.EstimatedSize, RegistryRoot: root, RegistrySubpath: record.RegistrySubpath,
		IsSystemComponent: record.IsSystemComponent, UninstallerMissing: record.UninstallerMissing,
	}
}

func appLower(text string) string { return cases.Lower(language.Und).String(text) }

// EnrichWindowsAppInventory is shared by registered and discovered inventories.
// Measurements and usage signals are display facts, never deletion authority.
func EnrichWindowsAppInventory(ctx context.Context, apps []InstalledApp, activity platform.AppActivityIndex, shortcuts map[string]uint64) []InstalledApp {
	apps = mergeAppRegistrations(apps)
	jobs := make(chan int)
	var workers sync.WaitGroup
	for range min(max(runtime.GOMAXPROCS(0), 2), 8, len(apps)) {
		workers.Go(func() {
			for index := range jobs {
				app := &apps[index]
				used := appLastUsed(*app, activity, shortcuts)
				if location := DeduceInstallLocation(*app); location != nil {
					used = max(used, activity.LookupOne(appLower(*location)), activity.LookupOne(appLower(filepath.Base(*location))))
					size, accessed := measureAppDirectory(ctx, *location)
					used = max(used, accessed)
					if app.InstallDate == nil {
						if info, err := os.Stat(*location); err == nil {
							created, _ := platform.AppFileTimes(info)
							date := created.Local().Format("2006-01-02")
							app.InstallDate = &date
							app.InstallDateRaw, _ = strconv.ParseUint(created.Local().Format("20060102"), 10, 64)
						}
					}
					app.EstimatedSize = max(app.EstimatedSize, size)
					if app.InstallLocation == nil {
						app.InstallLocation = location
					}
				}
				app.UninstallerMissing = appUninstallerMissing(*app)
				if used > 0 {
					app.LastUsedRaw = used
					date := time.Unix(int64(used), 0).Local().Format("2006-01-02")
					app.LastUsedDate = &date
				}
			}
		})
	}
send:
	for index := range apps {
		select {
		case <-ctx.Done():
			break send
		case jobs <- index:
		}
	}
	close(jobs)
	workers.Wait()
	return mergeAppLocations(apps)
}

func mergeAppRegistrations(apps []InstalledApp) []InstalledApp {
	byName := make(map[string]int)
	out := make([]InstalledApp, 0, len(apps))
	for _, app := range apps {
		key := appLower(app.Name) + "_" + appLower(app.Version)
		if app.Discovery != nil {
			key = app.ID
		}
		index, ok := byName[key]
		if !ok {
			byName[key] = len(out)
			out = append(out, app)
			continue
		}
		existing := &out[index]
		if existing.EstimatedSize == 0 {
			existing.EstimatedSize = app.EstimatedSize
		}
		if existing.UninstallString == nil {
			existing.UninstallString = app.UninstallString
		}
		if existing.InstallLocation == nil {
			existing.InstallLocation = app.InstallLocation
		}
		if existing.DisplayIcon == nil {
			existing.DisplayIcon = app.DisplayIcon
		}
		if existing.InstallDate == nil {
			existing.InstallDate, existing.InstallDateRaw = app.InstallDate, app.InstallDateRaw
		}
	}
	return out
}

func mergeAppLocations(apps []InstalledApp) []InstalledApp {
	locations := make(map[string]int)
	out := make([]InstalledApp, 0, len(apps))
	for _, app := range apps {
		if app.Discovery != nil || app.InstallLocation == nil || IsSystemRootDir(*app.InstallLocation) {
			out = append(out, app)
			continue
		}
		location := *app.InstallLocation
		index, ok := locations[location]
		if !ok {
			locations[location] = len(out)
			out = append(out, app)
			continue
		}
		existing := &out[index]
		// The reference compares version strings lexically, not as semver.
		if app.Version > existing.Version || existing.Version == "" && app.Version != "" {
			if app.EstimatedSize == 0 {
				app.EstimatedSize = existing.EstimatedSize
			}
			*existing = app
		} else if existing.EstimatedSize == 0 {
			existing.EstimatedSize = app.EstimatedSize
		}
	}
	return out
}

func DeduceInstallLocation(app InstalledApp) *string {
	if app.Discovery != nil {
		return app.InstallLocation
	}
	if app.InstallLocation != nil {
		if _, err := os.Stat(*app.InstallLocation); err == nil && !IsSystemRootDir(*app.InstallLocation) {
			return app.InstallLocation
		}
	}
	parentOfExisting := func(path string) *string {
		if _, err := os.Stat(path); err == nil {
			parent := filepath.Dir(path)
			if parent != path && !IsSystemRootDir(parent) {
				return &parent
			}
		}
		return nil
	}
	if app.DisplayIcon != nil {
		icon, _, _ := strings.Cut(*app.DisplayIcon, ",")
		if parent := parentOfExisting(strings.TrimSpace(strings.Trim(icon, "\""))); parent != nil {
			return parent
		}
	}
	if app.UninstallString != nil && !strings.Contains(appLower(*app.UninstallString), "msiexec") {
		if parts := ParseCmdLine(*app.UninstallString); len(parts) > 0 {
			if parent := parentOfExisting(parts[0]); parent != nil {
				return parent
			}
		}
	}
	candidates := []string{}
	if local, ok := platform.UserCacheDir(); ok {
		candidates = append(candidates, filepath.Join(local, "Programs", strings.TrimSpace(app.Name)))
	}
	if roaming, ok := platform.UserDataDir(); ok {
		candidates = append(candidates, filepath.Join(roaming, strings.TrimSpace(app.Name)))
	}
	candidates = append(candidates, filepath.Join(`C:\Program Files`, strings.TrimSpace(app.Name)), filepath.Join(`C:\Program Files (x86)`, strings.TrimSpace(app.Name)))
	for _, path := range candidates {
		if info, err := os.Stat(path); err == nil && info.IsDir() && !IsSystemRootDir(path) {
			return &path
		}
	}
	return nil
}

func appUninstallerMissing(app InstalledApp) bool {
	if app.Discovery != nil {
		if app.Discovery.Uninstaller == nil {
			return false
		}
		info, err := os.Stat(app.Discovery.Uninstaller.Executable)
		return err != nil || !info.Mode().IsRegular()
	}
	command := app.QuietUninstallString
	if command == nil {
		command = app.UninstallString
	}
	if command == nil {
		return true
	}
	if strings.Contains(appLower(*command), "msiexec") {
		return false
	}
	executable, _ := SplitCommand(*command)
	if executable == "" {
		return true
	}
	if !strings.ContainsAny(executable, `\/`) {
		return false
	}
	_, err := os.Stat(executable)
	return err != nil
}

func appLastUsed(app InstalledApp, activity platform.AppActivityIndex, shortcuts map[string]uint64) uint64 {
	tokens := platform.ExtractAppTokens(app.Name)
	used := max(activity.Lookup(tokens), activity.LookupOne(appLower(app.Name)), shortcuts[appLower(app.Name)])
	for _, token := range tokens {
		used = max(used, shortcuts[token])
	}
	lookupExecutable := func(path string) {
		path = appLower(path)
		base := path[strings.LastIndexAny(path, `\/`)+1:]
		stem := strings.TrimSuffix(base, filepath.Ext(base))
		used = max(used, activity.LookupOne(path), activity.LookupOne(base), activity.LookupOne(stem))
	}
	if app.DisplayIcon != nil {
		if path, ok := DisplayIconPath(*app.DisplayIcon); ok {
			lookupExecutable(path)
		}
	}
	for _, command := range []*string{app.UninstallString, app.QuietUninstallString} {
		if command != nil && !strings.Contains(appLower(*command), "msiexec") {
			if executable, _ := SplitCommand(*command); executable != "" {
				lookupExecutable(executable)
			}
		}
	}
	return used
}

func measureAppDirectory(ctx context.Context, root string) (size, newest uint64) {
	indexed, known := indexedDirectorySize(ctx, root)
	if known {
		size = indexed
	}
	_ = filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err != nil {
			return nil
		}
		relative, relErr := filepath.Rel(root, path)
		if known && entry.IsDir() && relErr == nil && relative != "." && strings.Count(relative, string(filepath.Separator)) >= 1 {
			return filepath.SkipDir
		}
		if !entry.Type().IsRegular() {
			return nil
		}
		info, err := entry.Info()
		if err != nil || !info.Mode().IsRegular() {
			return nil
		}
		if !known {
			size += uint64(info.Size())
		}
		if relErr == nil && (relative == "." || strings.Count(relative, string(filepath.Separator))+1 <= 2) {
			_, accessed := platform.AppFileTimes(info)
			if accessed.Unix() > 0 {
				newest = max(newest, uint64(accessed.Unix()))
			}
		}
		return nil
	})
	return size, newest
}
