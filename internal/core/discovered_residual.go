package core

import (
	"context"
	"errors"
	"path/filepath"
	"slices"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// validateDiscoveredResidualClean grants authority only for the supplied reviewed
// items. It does not assert global application absence or replace deletion-time
// identity checks. Live inspection may shrink ownership, never expand the review.
func validateDiscoveredResidualClean(ctx context.Context, app InstalledApp, items []ResidualItem, inspect func(string, *RuleSnapshot) *InstalledApp, processes func() ([]platform.AppRunningProcess, error)) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	d := app.Discovery
	if d == nil || d.Rule == nil || d.Rule.snapshot == nil || d.Rule.origin == nil {
		return errors.New("missing discovered source installation authority")
	}
	ref := *d.Rule
	if err := ref.Revalidate(); err != nil {
		return err
	}
	definition := ref.snapshot.Definition(ref.id)
	if definition == nil || definition.App == nil {
		return errors.New("missing source layout rule")
	}
	rule := *definition.App
	// The home is captured by the exact source-layout rule, not inferred from an
	// arbitrary executable's parent. Unknown portable evidence grants no home.
	home := ref.scope
	code := filepath.Join(home, filepath.FromSlash(rule.Source))
	if home == "" || !filepath.IsAbs(home) || !strings.HasPrefix(app.ID, "discovered:") || Norm(strings.TrimPrefix(app.ID, "discovered:")) != Norm(code) {
		return errors.New("discovered identity does not match source layout")
	}
	for _, item := range items {
		if item.Rule == nil || item.Rule.origin != ref.origin || item.Rule.id != ref.id || Norm(item.Rule.scope) != Norm(home) {
			return errors.New("residuals belong to different rule snapshots or installations")
		}
		if err := item.Rule.Revalidate(); err != nil {
			return err
		}
	}
	live := inspect(d.Executable, ref.snapshot)
	if live == nil || live.ID != app.ID || live.Discovery == nil || live.Discovery.Rule == nil || live.Discovery.Rule.id != ref.id || live.Discovery.Rule.origin != ref.snapshot {
		return errors.New("source installation ownership changed since residual scan")
	}
	for _, item := range items {
		if item.Source != ResidualInstallDir {
			continue
		}
		path := item.Kind.Path
		exact := func(paths []string) bool {
			return slices.ContainsFunc(paths, func(p string) bool { return Norm(p) == Norm(path) })
		}
		if (item.Kind.Type != ResidualFile && item.Kind.Type != ResidualDirectory) || !exact(d.ProgramPaths) || !exact(live.Discovery.ProgramPaths) {
			return errors.New("program ownership changed since residual scan")
		}
	}
	owners, err := processes()
	if err := sourceInstallIdle(home, rule, owners, err); err != nil {
		return err
	}
	return ctx.Err()
}
