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
	// 说人话：勾选不等于一定会删，删除前还会再核一次，任一项不通过就整批不删。
	r.apps.ResidualNotice = core.NewText(
		"勾选的项目在删除前会再确认一次：应用确实已卸载、文件没有被替换、当前没被占用；任一项不通过就整批不删。",
		"Before deleting, each selected item is checked again: the app is really gone, the file has not been replaced, and nothing is using it. If any check fails, nothing is deleted.",
	)
	if app != nil && app.Discovery != nil {
		r.apps.ResidualNotice = core.NewText(
			"这是来源安装扫描出的项目：只清理扫描时冻结、且归属与占用复核通过的所选项目。",
			"These came from a source-install scan: only selected targets frozen at scan time that pass the live ownership and occupancy checks are cleaned.",
		)
	}
}

// closeResiduals drops the review and its explanation together. The explanation
// describes the panel, so keeping it painted an unrelated banner on the page body.
func (r *Root) closeResiduals() {
	r.apps.ResidualsOpen = false
	r.apps.Residuals = nil
	r.apps.ResidualSize, r.apps.SelectedSize = "0 B", "0 B"
	r.apps.Occupancy = core.ResidualOccupancy{}
	r.apps.ResidualNotice = core.Text{}
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
			// Rust keeps the review closed after a clean: what survived is usually what
			// cannot be deleted, so reopening turns one clean into one popup per click.
			// The counts are reported above and the scan entry can list leftovers again.
			r.residual.Items = remaining
			r.closeResiduals()
		}
	})
}
