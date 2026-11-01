package cli

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func executeJunk(ctx context.Context, items []core.ScanItem, selected []bool, bytes uint64) *cleanupReport {
	var targets []core.CleanTarget
	var paths []string
	var files uint64
	for i, item := range items {
		if selected[i] {
			targets = append(targets, item.CleanTarget())
			paths = append(paths, item.Path)
			files += item.FileCount
		}
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
	report := core.CleanTargets(targets, p)
	result := cleanupOutcome(report, p)
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
