package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// Portable launch evidence owns individual files, never their containing tree.
// Snapshot and identity checks here supplement, not replace, cleaner guards.
func validatePortableResidualClean(ctx context.Context, app InstalledApp, items []ResidualItem, canonical func(string) (string, bool), shortcutTarget func(string) (string, bool), processes func() ([]platform.AppRunningProcess, error)) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	d := app.Discovery
	if d == nil || d.Rule != nil || d.Plan != nil || d.Uninstaller != nil || app.RegistryRoot != AppUnregistered || app.RegistrySubpath != "" || len(d.ProgramPaths) != 1 {
		return errors.New("missing exact portable discovery evidence")
	}
	exe := d.Executable
	if !filepath.IsAbs(exe) || !strings.EqualFold(filepath.Ext(exe), ".exe") || app.ID != "discovered:"+Norm(exe) || Norm(d.ProgramPaths[0]) != Norm(exe) || IsProtectedResidualPath(exe) {
		return errors.New("portable identity does not match captured executable")
	}
	// Resolve only the captured path, including a missing leaf after cleanup;
	// resolution does not grant any ancestor deletion authority.
	resolved, ok := canonicalSourceTarget(exe, canonical)
	if !ok || Norm(resolved) != Norm(exe) {
		return errors.New("portable executable redirected or unavailable")
	}
	var origin *RuleSnapshot
	for _, item := range items {
		ref := item.Rule
		if ref == nil || ref.origin == nil || ref.snapshot == nil || ref.id != "residual-windows" || ref.scope != "" || len(ref.contributors) != 0 {
			return errors.New("missing frozen portable residual rule")
		}
		if origin != nil && ref.origin != origin {
			return errors.New("portable residuals belong to different rule snapshots")
		}
		origin = ref.origin
		if err := ref.Revalidate(); err != nil {
			return err
		}
		path := item.Kind.Path
		if item.OwnerBundleID != nil || item.Kind.Type != ResidualFile || !filepath.IsAbs(path) || IsProtectedResidualPath(path) {
			return errors.New("portable authority is limited to captured files")
		}
		switch item.Source {
		case ResidualInstallDir:
			if Norm(path) != Norm(exe) {
				return errors.New("portable program path was not captured")
			}
		case ResidualShortcut:
			if !slices.ContainsFunc(d.Shortcuts, func(p string) bool { return Norm(p) == Norm(path) }) || !strings.EqualFold(filepath.Ext(path), ".lnk") {
				return errors.New("portable shortcut was not captured")
			}
			target, ok := shortcutTarget(path)
			if !ok {
				return errors.New("portable shortcut target unavailable")
			}
			target, ok = canonicalSourceTarget(target, canonical)
			if !ok || Norm(target) != Norm(exe) {
				return errors.New("portable shortcut target changed")
			}
		default:
			return errors.New("portable discovery does not authorize data or registration cleanup")
		}
		info, err := os.Lstat(path)
		resolved, ok := canonical(path)
		if err != nil || !info.Mode().IsRegular() || ruleEvidenceLink(info) || !ok || Norm(resolved) != Norm(path) || item.Identity == nil || runtime.GOOS == "windows" && !item.Identity.hasStable || !item.Identity.Recheck(path) {
			return errors.New("portable file identity changed or unavailable")
		}
	}
	owners, err := processes()
	if err != nil {
		return err
	}
	name := filepath.Base(exe)
	for _, owner := range owners {
		if owner.Image != "" {
			image, known := canonical(owner.Image)
			if Norm(owner.Image) == Norm(exe) || known && Norm(image) == Norm(exe) {
				return errors.New("portable executable is still running")
			}
			if known {
				continue
			}
		}
		// Unreadable images only block relevant names, not every unrelated
		// inaccessible Windows process. No parent-directory process scope exists.
		if owner.Name == "" || strings.EqualFold(owner.Name, name) || strings.EqualFold(strings.TrimSuffix(strings.ToLower(owner.Name), ".exe"), strings.TrimSuffix(strings.ToLower(name), ".exe")) {
			return errors.New("portable process ownership is unknown")
		}
	}
	return ctx.Err()
}
