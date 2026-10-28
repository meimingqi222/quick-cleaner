//go:build darwin

package platform

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
	"unicode/utf8"
)

const macMDLSBatch = 256
const macApplicationQuery = `kMDItemContentType == "com.apple.application-bundle"`

// ReadMacAppRecords inventories ordinary bundles as HKCU, not unregistered apps.
// Rule-owned uninstall routes are deliberately attached by core after this read.
func ReadMacAppRecords(ctx context.Context) []AppRecord {
	return ReadMacAppRecordsSized(ctx, nil)
}

// ReadMacAppRecordsSized uses estimates only for sizes; bundle identity stays live.
func ReadMacAppRecordsSized(ctx context.Context, measure func(context.Context, string) (uint64, bool)) []AppRecord {
	home, _ := UserHome()
	var paths []macAppRoot
	for _, root := range macAppRoots(home) {
		if ctx.Err() != nil {
			break
		}
		entries, _ := os.ReadDir(root.path)
		for _, e := range entries {
			path := filepath.Join(root.path, e.Name())
			if filepath.Ext(path) != ".app" {
				continue
			}
			if info, err := os.Stat(path); err == nil && info.IsDir() {
				paths = append(paths, macAppRoot{path, root.system})
			}
		}
	}
	lastUsed := make(map[string]macAppDate)
	sizes := map[string]uint64{}
	for start := 0; start < len(paths) && ctx.Err() == nil; start += macMDLSBatch {
		end := min(start+macMDLSBatch, len(paths))
		var names []string
		for _, p := range paths[start:end] {
			names = append(names, p.path)
		}
		args := append([]string{"-raw", "-name", "kMDItemLastUsedDate"}, names...)
		out, complete := macInventoryCommand(ctx, 8*time.Second, "/usr/bin/mdls", args...)
		if complete {
			if values, ok := macMetadataValues(out.Stdout, len(names)); ok && out.OK {
				for i, v := range values {
					lastUsed[names[i]] = macMetadataDate(v)
				}
			}
			idOut, idOK := macInventoryCommand(ctx, 8*time.Second, "/usr/bin/mdls", append([]string{"-raw", "-name", "kMDItemCFBundleIdentifier"}, names...)...)
			if idOK && idOut.OK && home != "" {
				if values, ok := macMetadataValues(idOut.Stdout, len(names)); ok {
					for i, id := range values {
						if lastUsed[names[i]].raw == 0 && macKnownID(id) {
							lastUsed[names[i]] = macFallbackLastUsed(home, id)
						}
					}
				}
			}
		}
		for path, size := range macAppSizes(ctx, names, measure, macInventoryCommand) {
			sizes[path] = size
		}
	}
	apps := make([]AppRecord, len(paths))
	valid := make([]bool, len(paths))
	jobs := make(chan int)
	var workers sync.WaitGroup
	for range min(max(runtime.GOMAXPROCS(0), 1), 8) {
		workers.Go(func() {
			for i := range jobs {
				if ctx.Err() != nil {
					continue
				}
				p := paths[i]
				size, ok := sizes[p.path]
				if !ok {
					size = macBundleAllocated(ctx, p.path)
				}
				apps[i] = macParseApp(ctx, p, lastUsed[p.path], size)
				valid[i] = ctx.Err() == nil
			}
		})
	}
	for i := range paths {
		if ctx.Err() != nil {
			break
		}
		jobs <- i
	}
	close(jobs)
	workers.Wait()
	result := make([]AppRecord, 0, len(paths))
	for i, a := range apps {
		if valid[i] {
			result = append(result, a)
		}
	}
	macSortAppRecords(result)
	return result
}

func macAppSizes(ctx context.Context, paths []string, measure func(context.Context, string) (uint64, bool), run macBundleRunner) map[string]uint64 {
	sizes := map[string]uint64{}
	var unresolved []string
	for _, path := range paths {
		if ctx.Err() != nil {
			return sizes
		}
		if measure != nil {
			if size, ok := measure(ctx, path); ok {
				sizes[path] = size
				continue
			}
		}
		unresolved = append(unresolved, path)
	}
	if len(unresolved) > 0 && ctx.Err() == nil {
		out, complete := run(ctx, 8*time.Second, "/usr/bin/du", append([]string{"-sk"}, unresolved...)...)
		if complete {
			for path, size := range macParseDU(out.Stdout) {
				sizes[path] = size
			}
		}
	}
	return sizes
}

func macParseApp(ctx context.Context, p macAppRoot, used macAppDate, size uint64) AppRecord {
	name := strings.TrimSuffix(filepath.Base(p.path), ".app")
	id, version := macReadBundleMetadata(ctx, p.path, macInventoryCommand)
	app := AppRecord{ID: name, Name: name, Publisher: "macOS Application", RegistryRoot: "HKCU", InstallLocation: &p.path, LastUsedDate: used.date, LastUsedRaw: used.raw, EstimatedSize: size, IsSystemComponent: p.system}
	if id != nil {
		app.ID = *id
		app.RegistrySubpath = *id
	}
	if version != nil {
		app.Version = *version
	}
	if p.system {
		app.Publisher = "Apple"
		app.RegistryRoot = "SystemApp"
	}
	if info, err := os.Stat(p.path); err == nil {
		created, _ := AppFileTimes(info)
		if created.IsZero() {
			created = info.ModTime()
		}
		date := macSystemDate(created)
		app.InstallDate = date.date
		app.InstallDateRaw = date.raw
	}
	app.UninstallString = macGenericUninstaller(p.path)
	app.UninstallerMissing = app.UninstallString == nil
	return app
}

func macBundleAllocated(ctx context.Context, path string) uint64 {
	// WalkDir follows a symlink used as its root in the Rust inventory, but not
	// links encountered inside a bundle. Go needs the root resolved explicitly.
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		path = resolved
	}
	var size uint64
	_ = filepath.WalkDir(path, func(_ string, e fs.DirEntry, err error) error {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err != nil {
			return nil
		}
		if e.Type().IsRegular() {
			if info, err := e.Info(); err == nil {
				if stat, ok := info.Sys().(*syscall.Stat_t); ok && stat.Blocks > 0 {
					size += uint64(stat.Blocks) * 512
				}
			}
		}
		return nil
	})
	return size
}

func macRootIDs(ctx context.Context, roots []macAppRoot) (map[string]struct{}, bool) {
	ids := make(map[string]struct{})
	for _, root := range roots {
		if ctx.Err() != nil {
			return nil, false
		}
		entries, err := os.ReadDir(root.path)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, false
		}
		for _, entry := range entries {
			path := filepath.Join(root.path, entry.Name())
			if !macRealBundle(path) {
				continue
			}
			id := macPlistString(ctx, filepath.Join(path, "Contents", "Info.plist"), "CFBundleIdentifier", macInventoryCommand)
			if id == nil || !macKnownID(*id) {
				return nil, false
			}
			ids[*id] = struct{}{}
		}
	}
	return ids, ctx.Err() == nil
}

func macRegistrationIDs(ctx context.Context) (map[string]struct{}, bool) {
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
	return macRegisteredLive(ctx, records, filepath.Join(home, ".Trash"), func(path string) *string { return macPlistString(ctx, path, "CFBundleIdentifier", macInventoryCommand) })
}

// InstalledBundleIDs returns known=false for any incomplete identity round.
// A partial set must never authorize orphan cleanup.
func InstalledBundleIDs(ctx context.Context) (map[string]struct{}, bool) {
	out, ok := macInventoryCommand(ctx, 8*time.Second, "/usr/bin/mdfind", "-0", macApplicationQuery)
	if !ok || !out.OK || strings.TrimSpace(string(out.Stderr)) != "" || !utf8.Valid(out.Stdout) {
		return nil, false
	}
	var paths []string
	for path := range strings.SplitSeq(string(out.Stdout), "\x00") {
		if path != "" && macRealBundle(path) {
			paths = append(paths, path)
		}
	}
	if len(paths) == 0 {
		return nil, false
	}
	ids := make(map[string]struct{})
	for start := 0; start < len(paths); start += macMDLSBatch {
		chunk := paths[start:min(start+macMDLSBatch, len(paths))]
		out, ok = macInventoryCommand(ctx, 8*time.Second, "/usr/bin/mdls", append([]string{"-raw", "-name", "kMDItemCFBundleIdentifier"}, chunk...)...)
		if !ok || !out.OK {
			return nil, false
		}
		batch, known := macIDsFromMetadata(chunk, out.Stdout, func(path string) *string { return macPlistString(ctx, path, "CFBundleIdentifier", macInventoryCommand) })
		if !known {
			return nil, false
		}
		for id := range batch {
			ids[id] = struct{}{}
		}
	}
	home, _ := UserHome()
	direct, ok := macRootIDs(ctx, macAppRoots(home))
	if !ok {
		return nil, false
	}
	for id := range direct {
		ids[id] = struct{}{}
	}
	registered, ok := macRegistrationIDs(ctx)
	if !ok {
		return nil, false
	}
	for id := range registered {
		ids[id] = struct{}{}
	}
	return ids, ctx.Err() == nil
}

// BundleIsStillInstalled distinguishes absence from an unavailable probe. It
// never caches absence; a stale LaunchServices path is not proof of removal.
func BundleIsStillInstalled(ctx context.Context, id string) (installed bool, known bool) {
	if !macKnownID(id) || ctx.Err() != nil {
		return false, false
	}
	home, _ := UserHome()
	roots := []macAppRoot{{"/Library/Input Methods", false}}
	if home != "" {
		roots = append(roots, macAppRoot{filepath.Join(home, "Library/Input Methods"), false})
	}
	ids, ok := macRootIDs(ctx, roots)
	if !ok {
		return false, false
	}
	if _, ok = ids[id]; ok {
		return true, true
	}
	out, ok := macInventoryCommand(ctx, 2*time.Second, "/usr/bin/mdfind", macBundleQuery(id))
	if !ok || !out.OK || strings.TrimSpace(string(out.Stderr)) != "" {
		return false, false
	}
	if macAnyInstallation(string(out.Stdout), home) {
		return true, true
	}
	out, ok = macInventoryCommand(ctx, 2*time.Second, "/usr/bin/mdfind", "-count", macApplicationQuery)
	if !ok || !out.OK || strings.TrimSpace(string(out.Stderr)) != "" {
		return false, false
	}
	count, err := strconv.ParseUint(strings.TrimSpace(string(out.Stdout)), 10, 64)
	if err != nil || count == 0 {
		return false, false
	}
	paths, ok := macWorkspacePaths(ctx, id)
	if !ok || home == "" {
		return false, false
	}
	live, ok := macRegisteredLive(ctx, map[string][]string{strings.ToLower(id): paths}, filepath.Join(home, ".Trash"), func(path string) *string { return macPlistString(ctx, path, "CFBundleIdentifier", macInventoryCommand) })
	if !ok {
		return false, false
	}
	return len(live) > 0, ctx.Err() == nil
}
