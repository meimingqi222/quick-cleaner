package core

import "context"

type macResidualCleaner struct {
	needsPrivilege func(string) bool
	elevated       func(context.Context, []string, func(string) bool) map[string]bool
	bootout        func(context.Context, string)
	deactivate     func(context.Context, ResidualKind) CleanResult
	dispose        func(string, Disposal, *CleanProgress) CleanResult
}

func cleanMacResidualsWith(ctx context.Context, items []ResidualItem, progress *CleanProgress, spot map[string]SpotCheck, native macResidualCleaner) CleanReport {
	var report CleanReport
	var origin *RuleSnapshot
	for _, item := range items {
		if item.Rule == nil {
			continue
		}
		if origin == nil {
			origin = item.Rule.origin
		}
		if item.Rule.origin != origin || item.Rule.Revalidate() != nil {
			for _, rejected := range items {
				report.RecordTarget(CleanFailure{ID: rejected.Kind.DisplayLabel()}, CleanFailed)
			}
			return report
		}
	}
	var elevated []string
	identities := map[string]*TargetIdentity{}
	for _, item := range items {
		if ctx.Err() != nil || progress.Cancelled() {
			break
		}
		switch item.Kind.Type {
		case ResidualFile, ResidualDirectory:
			path := item.Kind.Path
			progress.Note(path)
			state, known := spot[path]
			if !known || state != SpotClear {
				reason := FailUnverified
				if state == SpotBusy && known {
					reason = FailInUse
				}
				report.Record(path, progress.fail(path, reason))
				continue
			}
			if item.Identity == nil || !item.Identity.Recheck(path) {
				report.Record(path, progress.fail(path, FailChanged))
				continue
			}
			if IsElevatedResidualTarget(path) || native.needsPrivilege(path) {
				elevated = append(elevated, path)
				identities[path] = item.Identity
				continue
			}
			if item.Source == ResidualLaunchAgent {
				native.bootout(ctx, path)
			}
			// Bootout can execute a job's shutdown handler. Revalidate again rather
			// than granting that handler authority to replace the selected target.
			if ctx.Err() != nil || progress.Cancelled() {
				break
			}
			if !item.Identity.Recheck(path) {
				report.Record(path, progress.fail(path, FailChanged))
				continue
			}
			report.Record(path, native.dispose(path, RecycleBin, progress))
		case ResidualSystemExtension:
			report.RecordTarget(CleanFailure{ID: item.Kind.BundleID}, native.deactivate(ctx, item.Kind))
		}
	}
	if len(elevated) > 0 && ctx.Err() == nil && !progress.Cancelled() {
		removed := native.elevated(ctx, elevated, func(path string) bool {
			identity := identities[path]
			return ctx.Err() == nil && !progress.Cancelled() && IsElevatedResidualTarget(path) && identity != nil && identity.Recheck(path)
		})
		for _, path := range elevated {
			if removed[path] {
				report.Record(path, CleanOK)
			} else {
				report.Record(path, progress.fail(path, FailUnverified))
			}
		}
	}
	return report
}
