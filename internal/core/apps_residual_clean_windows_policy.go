package core

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf16"
)

type windowsResidualCleaner struct {
	pending     func() []uint16
	terminate   func([]string, func(string) bool)
	stopService func(string)
	absent      func(ResidualKind) (bool, bool)
	remove      func(ResidualKind) error
	dispose     func(string, Disposal, *CleanProgress) CleanResult
	wait        func(context.Context, time.Duration)
}

// ParseWindowsPendingRenamePaths retains every nonempty entry, including entries
// after empty rename destinations. Windows can lock either rename endpoint.
func ParseWindowsPendingRenamePaths(words []uint16) map[string]bool {
	out := make(map[string]bool)
	for start := 0; start < len(words); {
		end := start
		for end < len(words) && words[end] != 0 {
			end++
		}
		text := string(utf16.Decode(words[start:end]))
		start = end + 1
		if strings.HasPrefix(text, "*") {
			if i := strings.IndexByte(text, '\\'); i >= 0 {
				text = text[i+1:]
			}
		}
		text = strings.TrimLeft(text, `\`)
		text = strings.TrimPrefix(text, `??\`)
		if path := Norm(text); path != "" {
			out[path] = true
		}
	}
	return out
}

func windowsResidualCleanPlan(item ResidualItem) *CleanupPlan {
	rule := EngineRuleRef()
	if item.Rule != nil {
		rule = *item.Rule
	}
	op := item.Kind.Operation()
	path, disposal := item.Kind.Path, RecycleBin
	if op.Kind == OpNative {
		path, disposal = op.Identifier, Permanent
	}
	return NewCleanupPlan(rule, []PlannedTarget{{Path: path, Operation: op, Identity: item.Identity, Disposal: disposal}})
}

func cleanWindowsResidualsWith(ctx context.Context, items []ResidualItem, progress *CleanProgress, native windowsResidualCleaner) CleanReport {
	var report CleanReport
	if progress == nil {
		progress = NewCleanProgress(0, 0)
	}
	stop := context.AfterFunc(ctx, progress.RequestCancel)
	defer stop()
	if ctx.Err() != nil || progress.Cancelled() {
		return report
	}
	var origin *RuleSnapshot
	for _, item := range items {
		if item.Rule == nil {
			continue
		}
		if origin == nil {
			origin = item.Rule.origin
		}
		if origin != item.Rule.origin || item.Rule.Revalidate() != nil {
			for _, rejected := range items {
				report.RecordTarget(windowsResidualFailure(rejected.Kind), progress.fail(rejected.Kind.DisplayLabel(), FailUnverified))
			}
			return report
		}
	}
	locked := ParseWindowsPendingRenamePaths(native.pending())
	plans := make([]*CleanupPlan, len(items))
	var scopes []string
	for n, item := range items {
		plans[n] = windowsResidualCleanPlan(item)
		if item.Kind.Type != ResidualFile && item.Kind.Type != ResidualDirectory {
			continue
		}
		if locked[Norm(item.Kind.Path)] || IsProtectedResidualPath(item.Kind.Path) || plans[n].Validate() != nil {
			continue
		}
		path := item.Kind.Path
		if item.Kind.Type == ResidualFile {
			path = filepath.Dir(path)
		}
		if len(Norm(path)) > 3 && !IsProtectedResidualPath(path) {
			scopes = append(scopes, Norm(path))
		}
	}
	if len(scopes) > 0 && ctx.Err() == nil && !progress.Cancelled() {
		native.terminate(scopes, func(image string) bool {
			if ctx.Err() != nil || progress.Cancelled() {
				return false
			}
			for n, item := range items {
				if item.Kind.Type != ResidualFile && item.Kind.Type != ResidualDirectory {
					continue
				}
				scope := item.Kind.Path
				if item.Kind.Type == ResidualFile {
					scope = filepath.Dir(scope)
				}
				if !IsProtectedResidualPath(scope) && !locked[Norm(item.Kind.Path)] && atOrUnder(Norm(image), Norm(scope)) && plans[n].Validate() == nil {
					return true
				}
			}
			return false
		})
		native.wait(ctx, 250*time.Millisecond)
	}
	for n, item := range items {
		if ctx.Err() != nil || progress.Cancelled() {
			return report
		}
		if item.Kind.Type == ResidualRegistryKey && (item.Source == ResidualService || item.Source == ResidualLikelyService) && plans[n].Validate() == nil {
			if i := strings.LastIndexByte(item.Kind.Path, '\\'); i >= 0 && i+1 < len(item.Kind.Path) {
				native.stopService(item.Kind.Path[i+1:])
			}
		}
	}
	for n, item := range items {
		if ctx.Err() != nil || progress.Cancelled() {
			break
		}
		progress.Note(item.Kind.Path)
		if item.Kind.Type == ResidualSystemExtension {
			report.RecordTarget(windowsResidualFailure(item.Kind), CleanSkipped)
			continue
		}
		if (item.Kind.Type == ResidualFile || item.Kind.Type == ResidualDirectory) && locked[Norm(item.Kind.Path)] {
			report.Record(item.Kind.Path, CleanManualAction)
			continue
		}
		executor := &windowsResidualCleanExecutor{filesystemExecutor: filesystemExecutor{progress: progress}, ctx: ctx, plan: plans[n], item: item, native: native}
		outcome := executeCapability(plans[n], 0, progress, SpotClear, executor)
		if item.Kind.Type != ResidualFile && item.Kind.Type != ResidualDirectory {
			for _, list := range []*[]CleanFailure{&outcome.Failed, &outcome.SkippedItems, &outcome.Manual} {
				for n := range *list {
					(*list)[n] = windowsResidualFailure(item.Kind)
				}
			}
		}
		report.Merge(outcome)
	}
	return report
}

func windowsResidualFailure(kind ResidualKind) CleanFailure {
	if kind.Type == ResidualFile || kind.Type == ResidualDirectory {
		return CleanFailure{Path: kind.Path}
	}
	return CleanFailure{ID: kind.DisplayLabel()}
}

type windowsResidualCleanExecutor struct {
	filesystemExecutor
	ctx    context.Context
	plan   *CleanupPlan
	item   ResidualItem
	native windowsResidualCleaner
	err    string
}

func (e *windowsResidualCleanExecutor) supports(t PlannedTarget) bool {
	return t.Operation.Kind == OpFile || t.Operation.Kind == OpTree || t.Operation.Kind == OpNative && (e.item.Kind.Type == ResidualRegistryKey || e.item.Kind.Type == ResidualRegistryValue || e.item.Kind.Type == ResidualScheduledTask)
}

// Windows relies on sharing violations and the live-database gate in Dispose;
// the Rust Windows channel deliberately supplies SpotClear rather than probing.
func (*windowsResidualCleanExecutor) requiresOccupancy() bool { return false }
func (e *windowsResidualCleanExecutor) prepare(t PlannedTarget) (string, error) {
	if e.ctx.Err() != nil {
		return "", e.ctx.Err()
	}
	if t.Operation.Kind != OpNative && IsProtectedResidualPath(t.Path) {
		return "", errors.New("Protected residual target")
	}
	return "", nil
}
func (e *windowsResidualCleanExecutor) apply(t PlannedTarget) CleanReport {
	var report CleanReport
	if e.ctx.Err() != nil || e.progress.Cancelled() {
		report.Record(t.Path, CleanSkipped)
		return report
	}
	// Process/service shutdown may run arbitrary application shutdown handlers.
	// Revalidate immediately before applying, not merely before that shutdown.
	if err := e.plan.Validate(); err != nil {
		e.err = err.Error()
		report.Record(t.Path, e.progress.fail(t.Path, FailChanged))
		return report
	}
	if t.Operation.Kind != OpNative {
		report.Record(t.Path, e.native.dispose(t.Path, RecycleBin, e.progress))
		return report
	}
	absent, known := e.native.absent(e.item.Kind)
	if !known {
		e.err = "Native state unknown before deletion; refused"
	} else if absent {
		report.Record(t.Path, CleanOK)
		return report
	} else {
		if e.ctx.Err() != nil || e.progress.Cancelled() {
			report.Record(t.Path, CleanSkipped)
			return report
		}
		deleteErr := e.native.remove(e.item.Kind)
		absent, known = e.native.absent(e.item.Kind)
		if known && absent {
			report.Record(t.Path, CleanOK)
			return report
		}
		e.err = "Native removal did not establish verified absence"
		if deleteErr != nil {
			e.err = fmt.Sprintf("%s: %v", e.err, deleteErr)
		}
	}
	report.Record(t.Path, e.progress.fail(t.Path, FailUnverified))
	return report
}
func (e *windowsResidualCleanExecutor) verify(c CompletionCondition) Evidence {
	if e.ctx.Err() != nil {
		return Unknown
	}
	if e.item.Kind.Type == ResidualFile || e.item.Kind.Type == ResidualDirectory {
		return FilesystemCompletion(c)
	}
	absent, known := e.native.absent(e.item.Kind)
	if !known {
		return Unknown
	}
	if absent {
		return Confirmed
	}
	return Absent
}
func (e *windowsResidualCleanExecutor) failureReason() string { return e.err }

func windowsResidualWait(ctx context.Context, duration time.Duration) {
	timer := time.NewTimer(duration)
	defer timer.Stop()
	select {
	case <-ctx.Done():
	case <-timer.C:
	}
}
