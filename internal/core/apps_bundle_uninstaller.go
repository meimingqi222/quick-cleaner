package core

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func MacAppUninstaller(bundlePath, bundleID string) *string {
	if spec := ReviewedScriptUninstaller(bundleID); spec != nil {
		id, _ := platform.ReadBundleMetadata(filepath.Join(bundlePath, "Contents", "Info.plist"))
		if id == nil || *id != spec.BundleID {
			return nil
		}
		path := filepath.Join(bundlePath, filepath.FromSlash(spec.Script))
		if confined, err := ConfinedRuleFile(bundlePath, path); err == nil && confined {
			return &path
		}
		return nil
	}
	snapshot, err := CurrentRules()
	if err != nil {
		return nil
	}
	if rule := snapshot.Definition("residual-macos"); rule != nil {
		if locations, declared := rule.Locations["embedded_uninstaller:"+bundleID]; declared {
			for _, location := range locations[:min(len(locations), 16)] {
				path := filepath.Join(bundlePath, filepath.FromSlash(location.Path))
				info, err := os.Stat(path)
				if err == nil && info.IsDir() && filepath.Ext(path) == ".app" {
					if confined, err := ConfinedRulePath(bundlePath, path); err == nil && confined {
						return &path
					}
				}
			}
			// A declared official route is mandatory: never guess another one.
			return nil
		}
	}
	resources := filepath.Join(bundlePath, "Contents", "Resources")
	entries, _ := os.ReadDir(resources)
	for _, entry := range entries {
		path := filepath.Join(resources, entry.Name())
		info, err := os.Stat(path)
		if err == nil && info.IsDir() && filepath.Ext(path) == ".app" && strings.HasPrefix(appLower(strings.TrimSuffix(entry.Name(), ".app")), "uninstall") {
			return &path
		}
	}
	return nil
}
