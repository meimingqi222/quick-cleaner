package app

import (
	"context"
	"fmt"
	"slices"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) showResiduals(result core.ResidualScanResult, app *core.InstalledApp) {
	r.residual, r.residualApp = result, app
	r.apps.ResidualsOpen = true
	r.apps.OrphanLeftovers = result.Scope == core.ResidualScopeOrphanLeftovers
	r.apps.ResidualAppName = result.AppName
	r.apps.Residuals = nil
	var total uint64
	selected := result.DefaultSelection()
	for i, item := range result.Items {
		_, picked := selected[i]
		total += item.Size()
		r.apps.Residuals = append(r.apps.Residuals, view.AppsPageResidual{ID: fmt.Sprint(i), Path: item.Kind.DisplayLabel(), Size: core.FmtSize(item.Size()), Confidence: item.Confidence, Source: item.Source, Kind: item.Kind.Type, Manual: item.Kind.Type == core.ResidualSystemExtension, Selected: picked})
	}
	r.apps.ResidualSize = core.FmtSize(total)
	r.residualSummary()
	r.apps.Occupancy = result.Occupancy
	r.apps.Notice = core.NewText("仅清理单独确认的残留；删除前重新验证应用不存在、目标身份及占用。", "Only separately confirmed residuals can be removed; application absence, target identity and occupancy are revalidated before disposal.")
	if app != nil && app.Discovery != nil {
		r.apps.Notice = core.NewText("来源安装仅允许清理冻结范围内、实时归属与占用复核通过的所选残留。", "Source-install cleanup is limited to selected frozen targets whose live ownership and vacancy are verified.")
	}
}

func (r *Root) residualSummary() {
	var bytes uint64
	for i, row := range r.apps.Residuals {
		if row.Selected && i < len(r.residual.Items) {
			bytes += r.residual.Items[i].Size()
		}
	}
	r.apps.SelectedSize = core.FmtSize(bytes)
}

func (r *Root) selectResiduals(mode int) {
	if r.busy() {
		return
	}
	recommended := r.residual.DefaultSelection()
	for i := range r.apps.Residuals {
		row := &r.apps.Residuals[i]
		_, picked := recommended[i]
		row.Selected = !row.Manual && (mode == 2 || mode == 1 && picked)
	}
	r.residualSummary()
}

func (r *Root) scanOrphanResiduals() {
	if r.busy() {
		return
	}
	r.apps.ResidualScanning = true
	r.start(false, nil, func(ctx context.Context) func() {
		result, err := scanOrphans(ctx)
		return func() {
			r.apps.ResidualScanning = false
			if err != nil {
				r.apps.Notice = core.SameText(err.Error())
				r.failure(err)
				return
			}
			if result == nil {
				r.failure(fmt.Errorf("orphan inventory returned no evidence"))
				return
			}
			r.showResiduals(*result, nil)
		}
	})
}

func (r *Root) requestResidualClean() {
	if r.busy() {
		return
	}
	var items []core.ResidualItem
	var labels []string
	for i, row := range r.apps.Residuals {
		if row.Selected && !row.Manual && i < len(r.residual.Items) {
			item := r.residual.Items[i]
			items = append(items, item)
			labels = append(labels, item.DisplayLabelLang(r.settings.Language))
		}
	}
	if len(items) == 0 {
		return
	}
	app, scope := r.residualApp, r.residual.Scope
	// Rust cleans the reviewed selection directly: ticking rows and pressing the button
	// already is the confirmation, and the extra dialog only delayed it. The batch is
	// still refused unless ownership, occupancy and absence evidence verify.
	r.apps.Cleaning = true
	p := core.NewCleanProgress(0, 0)
	r.start(true, p, func(ctx context.Context) func() {
		allowed := false
		if scope == core.ResidualScopeOrphanLeftovers {
			allowed = orphanAbsent(ctx, items)
		} else if app != nil {
			allowed = r.residualAuthority(ctx, *app, items)
		}
		if !allowed || ctx.Err() != nil || p.Cancelled() {
			return func() {
				r.apps.Cleaning = false
				r.apps.Notice = core.NewText("残留清理授权无法验证；未清理任何残留。", "Residual cleanup authority could not be verified; no residuals were cleaned.")
				r.record("clean_residuals", labels, 0, uint(len(items)), 0, 0)
			}
		}
		report := cleanResiduals(ctx, slices.Clone(items), p)
		// Verification is limited to reviewed candidates, never a fresh discovery.
		remaining, err := verifyResiduals(ctx, items)
		return func() {
			r.apps.Cleaning = false
			r.finishCleanup("clean_residuals", labels, report, p)
			if p.Cancelled() {
				return
			}
			if err != nil {
				r.apps.Notice = core.SameText(err.Error())
				return
			}
			// Keep unselected candidates alongside selected candidates that survived.
			selected := map[string]bool{}
			for _, item := range items {
				selected[item.DisplayLabel()] = true
			}
			for _, item := range r.residual.Items {
				if !selected[item.DisplayLabel()] {
					remaining = append(remaining, item)
				}
			}
			result := r.residual
			result.Items = remaining
			r.showResiduals(result, app)
		}
	})
}
