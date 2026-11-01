package cli

import (
	"context"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// Seams so tests never list or quit the user's real applications.
var (
	findRunningApps  = core.FindRunningApps
	closeRunningApps = func(apps []core.RunningApp, cancelled func() bool) []core.Text {
		return core.CloseRunningApps(apps, 8*time.Second, cancelled)
	}
)

func executeJunk(ctx context.Context, items []core.ScanItem, selected []bool, bytes uint64, closeApps bool) *cleanupReport {
	var chosen []core.ScanItem
	for i, item := range items {
		if selected[i] {
			chosen = append(chosen, item)
		}
	}
	// Headless runs cannot ask: without --close-apps a running owner app's
	// caches are left out; with it, the app is asked to quit normally first.
	apps := findRunningApps(chosen)
	leftOut := map[string]bool{}
	var running []runningAppReport
	if !closeApps {
		for _, app := range apps {
			for _, path := range app.Paths {
				leftOut[path] = true
			}
		}
	}
	var targets []core.CleanTarget
	var paths []string
	var files uint64
	for _, item := range chosen {
		if leftOut[item.Path] {
			bytes -= min(bytes, item.Size)
			continue
		}
		targets = append(targets, item.CleanTarget())
		paths = append(paths, item.Path)
		files += item.FileCount
	}
	p := core.NewCleanProgress(files, bytes)
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-ctx.Done():
			p.RequestCancel()
		case <-done:
		}
	}()
	if ctx.Err() != nil {
		p.RequestCancel()
	}
	var still []core.Text
	if closeApps && len(apps) > 0 {
		still = closeRunningApps(apps, p.Cancelled)
	}
	for _, app := range apps {
		name := app.Name.Get(core.English)
		stays := false
		for _, s := range still {
			stays = stays || s.Get(core.English) == name
		}
		running = append(running, runningAppReport{Name: name, Paths: app.Paths, Closed: closeApps, StillRunning: stays})
	}
	report := core.CleanTargets(targets, p)
	result := cleanupOutcome(report, p)
	result.RunningApps = running
	result.Skipped += uint(len(leftOut))
	// A run where every target was skipped as in use changed nothing; it is
	// reported but not recorded as a cleanup.
	if len(paths) > 0 && (result.OK > 0 || result.DeletedFiles > 0 || len(result.Failed)+len(result.Manual) > 0) {
		core.RecordHistory("cli_clean", paths, result.OK, result.Skipped, uint(len(result.Failed)+len(result.Manual)), result.FreedBytes)
	}
	return result
}

func cleanupOutcome(report core.CleanReport, p *core.CleanProgress) *cleanupReport {
	snap := p.Snapshot()
	failed, skipped := p.SplitFailures(report.Failed)
	result := &cleanupReport{
		OK: report.OK, Skipped: report.Skipped,
		Failed: failed, Manual: report.Manual, SkippedItems: append(append([]core.CleanFailure{}, report.SkippedItems...), skipped...),
		Remaining: report.RemainingLive, Executions: report.PlanExecutions,
		DeletedFiles: snap.Files, FreedBytes: snap.Bytes,
	}
	result.Skipped += uint(len(skipped))
	for _, info := range report.FailInfo {
		if info.FailedLeaves > 0 && info.FailedLeaves == info.SkippedLeaves {
			result.SkippedTargets = append(result.SkippedTargets, info)
		} else {
			result.Failures = append(result.Failures, info)
		}
	}
	return result
}
