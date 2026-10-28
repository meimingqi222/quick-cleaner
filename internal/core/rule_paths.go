package core

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// An empty root is unavailable, not permission to substitute the process user.
func UserRuleRoots(home string) map[string]string {
	roots := map[string]string{"home": home, "temp": os.TempDir()}
	if runtime.GOOS == "windows" {
		roots["local"], _ = platform.UserCacheDir()
		roots["roaming"], _ = platform.UserDataDir()
		roots["system"] = os.Getenv("SystemRoot")
		if roots["system"] == "" {
			roots["system"] = `C:\Windows`
		}
	} else if home != "" {
		roots["local"] = filepath.Join(home, "Library/Caches")
		roots["roaming"] = filepath.Join(home, "Library/Application Support")
	}
	roots["cache"] = roots["local"]
	roots["user_temp"], _ = platform.UserTempDir()
	return roots
}

func rulePlatform() string {
	if runtime.GOOS == "darwin" {
		return "macos"
	}
	return runtime.GOOS
}

// RuleTargets combines fixed, directory and version declarations before
// normalization, preserving constraints contributed by overlapping rules.
func RuleTargets(snapshot *RuleSnapshot, roots map[string]string, osName string) []ScanTarget {
	targets := RulePathTargets(snapshot, roots, osName)
	targets = append(targets, RuleDirectoryTargets(snapshot, roots, osName)...)
	targets = append(targets, RuleVersionTargets(snapshot, roots, osName)...)
	return NormalizeScanTargets(targets)
}

// RulePathTargets expands only fixed declarations. Directory selection and
// provider discovery are independent inputs to NormalizeScanTargets.
func RulePathTargets(snapshot *RuleSnapshot, roots map[string]string, osName string) []ScanTarget {
	var targets []ScanTarget
	for _, definition := range snapshot.Bundle.Rules {
		if definition.Platform != "all" && definition.Platform != osName {
			continue
		}
		observed := map[string]RuleRef{}
		for _, entry := range definition.Entries {
			root := roots[entry.Root]
			if root == "" && osName == "windows" && len(entry.Root) == 7 && strings.HasPrefix(entry.Root, "drive:") {
				root = entry.Root[6:] + ":/"
			}
			if root == "" {
				continue
			}
			ref, ok := observed[root]
			if !ok {
				ref = NewRuleRef(snapshot, definition.ID, root)
				observed[root] = ref
			}
			if ref.observation.Detected != Confirmed {
				continue
			}
			paths := []string{"."}
			if entry.Root != "user_temp" || entry.Path != "." {
				var err error
				paths, err = ExpandRulePaths(entry.Path, ref.observation.Variables)
				if err != nil {
					continue
				}
			}
			for _, relative := range paths {
				path := filepath.Join(root, strings.ReplaceAll(relative, `\`, "/"))
				if len(definition.Variables) > 0 {
					confined, err := ConfinedRulePath(root, path)
					if err != nil || !confined {
						continue
					}
				}
				protected := IsProtectedResidualPath(path)
				if entry.Operation == "contents" {
					protected = IsContentsProtected(path)
				}
				if protected {
					continue
				}
				oldEnough := false
				if md, err := os.Stat(path); err == nil {
					age := time.Since(md.ModTime())
					oldEnough = age >= 0 && uint64(age/time.Second) >= entry.MinimumAgeSeconds
				}
				targets = append(targets, ScanTarget{Operation: Operation{Kind: OperationKind(entry.Operation)}, Disposal: entry.Disposal, Rule: ref, Path: path, Label: NewText(entry.Zh, entry.En), Category: CategoryID(entry.Category), Recommended: entry.Recommended && (entry.MinimumAgeSeconds == 0 || oldEnough)})
			}
		}
	}
	return targets
}
