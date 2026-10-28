package core

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func scanWindowsResidualFiles(ctx context.Context, app InstalledApp, c windowsResidualContext, r windowsResidualReader, roots windowsResidualRoots) []ResidualItem {
	var out []ResidualItem
	push := func(path string, source ResidualSource, confidence Confidence, directory bool) {
		if ctx.Err() != nil {
			return
		}
		if item, ok := windowsResidualFile(ctx, path, source, confidence, directory); ok {
			out = append(out, item)
		}
	}
	if app.Discovery != nil {
		for _, path := range app.Discovery.ProgramPaths {
			push(path, ResidualInstallDir, Certain, false)
		}
		for _, path := range app.Discovery.Shortcuts {
			if md, err := os.Stat(path); err == nil && md.Mode().IsRegular() && r.ShortcutTargets(path, app.Discovery.Executable) {
				push(path, ResidualShortcut, Certain, false)
			}
		}
	} else if app.InstallLocation != nil {
		push(*app.InstallLocation, ResidualInstallDir, Certain, false)
		for p := filepath.Dir(*app.InstallLocation); p != filepath.Dir(p); p = filepath.Dir(p) {
			if ctx.Err() != nil || IsProtectedResidualPath(p) {
				break
			}
			if _, err := os.Stat(p); err == nil {
				hasFile := false
				_ = filepath.WalkDir(p, func(_ string, e fs.DirEntry, err error) error {
					if ctx.Err() != nil {
						return ctx.Err()
					}
					if err == nil && e.Type().IsRegular() {
						hasFile = true
						return fs.SkipAll
					}
					return nil
				})
				if hasFile {
					break
				}
				push(p, ResidualEmptyInstallParent, Certain, true)
			}
		}
	}
	if definition := c.snapshot.Definition("residual-windows"); definition != nil {
		for _, alias := range definition.AppDataAliases {
			if !AppRegistrationMatches(app, alias.RegistryID, alias.Name, alias.Publisher) {
				continue
			}
			for _, root := range roots.data {
				path := filepath.Join(root, alias.Directory)
				md, err := os.Lstat(path)
				confined, confErr := ConfinedRulePath(root, path)
				if err == nil && md.IsDir() && !ruleEvidenceLink(md) && confined && confErr == nil {
					push(path, ResidualAppDataDir, Possible, true)
				}
			}
		}
	}
	if c.matchable() {
		for _, root := range roots.data {
			push(filepath.Join(root, c.token), ResidualAppDataDir, Certain, true)
			if c.shortToken != "" {
				// The display name carried a version; the folder usually drops it.
				push(filepath.Join(root, c.shortToken), ResidualAppDataDir, Certain, true)
			}
			if IsSafeAppToken(c.publisher) {
				push(filepath.Join(root, c.publisher, c.token), ResidualAppDataDir, Certain, true)
			}
			for _, entry := range windowsResidualEntries(root, 4000) {
				if entry.IsDir() && !strings.EqualFold(entry.Name(), c.token) && c.matches(entry.Name()) {
					push(filepath.Join(root, entry.Name()), ResidualLikelyAppDataDir, Possible, true)
				}
			}
		}
		seen := make(map[string]bool)
		for _, root := range roots.shortcuts {
			if seen[Norm(root)] {
				continue
			}
			seen[Norm(root)] = true
			windowsResidualWalk(ctx, root, 4, 4000, func(path string, entry fs.DirEntry) {
				stem := strings.TrimSuffix(entry.Name(), ".lnk")
				if !c.matches(stem) {
					return
				}
				confidence := Possible
				if windowsResidualName(stem) == c.name {
					confidence = Certain
				}
				if entry.IsDir() {
					push(path, ResidualStartMenuDir, confidence, true)
				} else if strings.HasSuffix(asciiLower(entry.Name()), ".lnk") {
					push(path, ResidualShortcut, confidence, false)
				}
			})
		}
	}
	if c.install != "" || len(c.exes) > 0 {
		tasks := filepath.Join(roots.system, "System32", "Tasks")
		windowsResidualWalk(ctx, tasks, 6, 3999, func(path string, entry fs.DirEntry) {
			if !entry.Type().IsRegular() {
				return
			}
			rel, err := filepath.Rel(tasks, path)
			if err != nil {
				return
			}
			rel = strings.ReplaceAll(rel, string(filepath.Separator), `\`)
			if strings.HasPrefix(asciiLower(rel), `microsoft\windows\`) {
				return
			}
			data, err := os.ReadFile(path)
			if err != nil {
				return
			}
			text := windowsResidualTaskText(data)
			hitDir, hitExe := c.mentions(text), c.mentionsExe(text)
			if !hitDir && (!hitExe || windowsResidualSystemFile(asciiLower(text))) {
				return
			}
			confidence := Possible
			if hitDir {
				confidence = Certain
			}
			out = append(out, NewResidualItem(ResidualKind{Type: ResidualScheduledTask, Path: `\` + rel}, ResidualTaskSource, confidence))
		})
	}
	if len(c.exes) > 0 {
		prefetch := filepath.Join(roots.system, "Prefetch")
		for _, entry := range windowsResidualEntries(prefetch, -1) {
			exe := platform.WindowsResidualPrefetchExecutable(entry.Name())
			if exe != "" && slices.Contains(c.exes, exe) {
				push(filepath.Join(prefetch, entry.Name()), ResidualPrefetchFile, Certain, false)
			}
		}
		if roots.local != "" {
			dumps := filepath.Join(roots.local, "CrashDumps")
			for _, entry := range windowsResidualEntries(dumps, -1) {
				name := asciiLower(entry.Name())
				if strings.HasSuffix(name, ".dmp") && slices.ContainsFunc(c.exes, func(exe string) bool { return strings.HasPrefix(name, exe) }) {
					push(filepath.Join(dumps, entry.Name()), ResidualCrashDump, Certain, false)
				}
			}
		}
	}
	command := app.QuietUninstallString
	if command == nil {
		command = app.UninstallString
	}
	if command != nil && !strings.Contains(asciiLower(*command), "msiexec") {
		exe, _ := SplitCommand(*command)
		name := asciiLower(filepath.Base(exe))
		parent := filepath.Dir(exe)
		if exe != "" && parent != "." && Norm(parent) != c.install && (strings.HasPrefix(name, "unins") || strings.Contains(name, "uninstall") || strings.Contains(name, "uninst")) {
			push(parent, ResidualUninstallerLeftover, Certain, true)
		}
	}
	return out
}

func windowsResidualSystemFile(path string) bool {
	return strings.Contains(path, `\windows\system32\`) || strings.Contains(path, `\windows\syswow64\`) || strings.Contains(path, `\windows\winsxs\`)
}
