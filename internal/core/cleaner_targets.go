package core

import (
	"context"
	"log"
	"runtime"
	"strings"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type CleanTarget struct {
	Plans     []*CleanupPlan
	Rule      *RuleRef
	Operation Operation
	Path      string
	RemoveDir bool
	SizeHint  *uint64
	Identity  *TargetIdentity
	Disposal  Disposal
}

// Selection only carries authority captured during scanning, never new stat data.
func (item ScanItem) CleanTarget() CleanTarget {
	rule := item.Rule
	return CleanTarget{Plans: append([]*CleanupPlan(nil), item.Plans...), Rule: &rule, Operation: item.Operation, Path: item.Path, RemoveDir: item.Operation.RemovesDirectory(), SizeHint: &item.Size, Identity: item.Identity, Disposal: item.Disposal}
}

func MergeCleanTargets(targets []CleanTarget) []CleanTarget {
	var merged []CleanTarget
	positions := map[string]int{}
	for _, t := range targets {
		key := t.Operation.TargetKey(t.Path)
		if i, exists := positions[key]; exists {
			first := &merged[i]
			conflict := first.Operation != t.Operation || first.Disposal != t.Disposal || first.RemoveDir != t.RemoveDir || (first.Identity == nil) != (t.Identity == nil) || first.Identity != nil && !first.Identity.Recheck(first.Path) || t.Identity != nil && !t.Identity.Recheck(t.Path)
			for _, plan := range t.Plans {
				found := false
				for _, existing := range first.Plans {
					if plan == existing {
						found = true
						break
					}
				}
				if !found {
					first.Plans = append(first.Plans, plan)
				}
			}
			if t.Rule != nil {
				if first.Rule == nil {
					copy := *t.Rule
					first.Rule = &copy
				} else {
					combined := first.Rule.Merge(*t.Rule)
					first.Rule = &combined
				}
			}
			if conflict {
				rule := EngineRuleRef()
				if first.Rule != nil {
					rule = *first.Rule
				}
				rule.blocked = "Conflicting selected cleanup policies"
				first.Rule = &rule
			}
		} else {
			t.Plans = append([]*CleanupPlan(nil), t.Plans...)
			if t.Rule != nil {
				copy := *t.Rule
				t.Rule = &copy
			}
			positions[key] = len(merged)
			merged = append(merged, t)
		}
	}
	return merged
}

func CleanTargets(selected []CleanTarget, p *CleanProgress) CleanReport {
	targets := MergeCleanTargets(selected)
	paths := make([]string, len(targets))
	for i, t := range targets {
		paths[i] = t.Path
	}
	log.Printf("[删除] 分类清理，共 %d 个目标：%q", len(paths), paths)
	spot := SpotCheckInUse(paths)
	var report CleanReport
	trashDone := false
	for _, t := range targets {
		if p.Cancelled() {
			break
		}
		p.Note(t.Path)
		if IsManagedAgentWorktree(t.Path) {
			report.Record(t.Path, p.fail(t.Path, FailWorktreeManaged))
			continue
		}
		if t.Rule != nil {
			if err := t.Rule.Revalidate(); err != nil {
				report.Record(t.Path, p.fail(t.Path, FailUnverified))
				continue
			}
		}
		valid := t.RemoveDir == t.Operation.RemovesDirectory() && t.Operation.ValidateTarget(t.Path, t.RemoveDir)
		for _, plan := range t.Plans {
			if plan == nil || plan.ValidateBinding(t.Path, t.Operation, t.Disposal, t.Identity) != nil {
				valid = false
				break
			}
		}
		if !valid {
			report.Record(t.Path, p.fail(t.Path, FailUnverified))
			continue
		}
		occupancy, known := spot[t.Path]
		if !known {
			occupancy = SpotUnknown
		}
		if !t.Operation.IsNativeResource() && occupancy != SpotClear {
			reason := FailUnverified
			if occupancy == SpotBusy {
				reason = FailInUse
			}
			report.Record(t.Path, p.fail(t.Path, reason))
			continue
		}
		if t.Operation.Kind == OpTrash {
			if platform.IsVolumeTrash(t.Path) || platform.IsPerDriveTrash(t.Path) {
				report.Merge(CleanDirContents(t.Path, p))
				continue
			}
			if !trashDone {
				report.Merge(EmptyTrash(p))
				trashDone = true
			}
			continue
		}
		// Broken login items stay recoverable: Trash them even when the rule
		// disposal is permanent. Only a direct child of the two LaunchAgents
		// directories qualifies, and only after the occupancy gate above.
		if runtime.GOOS == "darwin" {
			home, _ := platform.UserHome()
			if launchAgentPlist(t.Path, home) {
				if !RootIdentityHolds(t.Path, t.Identity) {
					report.Record(t.Path, p.fail(t.Path, FailChanged))
					continue
				}
				report.Record(t.Path, recyclePathWith(t.Path, p, platform.MoveToTrash))
				continue
			}
		}
		var plan *CleanupPlan
		if len(t.Plans) > 0 {
			plan = t.Plans[0]
		} else if t.Rule != nil {
			plan = NewCleanupPlan(*t.Rule, []PlannedTarget{{Path: t.Path, Operation: t.Operation, Identity: t.Identity, Disposal: t.Disposal}})
		}
		index := -1
		if plan != nil {
			for i, target := range plan.targets {
				if target.Operation.TargetKey(target.Path) == t.Operation.TargetKey(t.Path) {
					index = i
					break
				}
			}
		}
		if index < 0 {
			report.Record(t.Path, p.fail(t.Path, FailUnverified))
			continue
		}
		switch t.Operation.Route() {
		case RouteFilesystem:
			report.Merge(ExecuteFilesystemPlan(plan, index, p, occupancy))
		case RouteOwner:
			size := uint64(0)
			if t.SizeHint != nil {
				size = *t.SizeHint
			}
			report.Merge(ExecuteOwnerPlan(plan, index, p, occupancy, size))
		case RouteWorktree:
			report.Merge(ExecuteWorktreePlan(plan, index, p, occupancy))
		case RouteNative:
			size := uint64(0)
			if t.SizeHint != nil {
				size = *t.SizeHint
			}
			report.Merge(ExecuteNativePlan(plan, index, p, size))
		default:
			report.Record(t.Path, p.fail(t.Path, FailUnverified))
		}
	}
	p.mu.Lock()
	reasons := make(map[string]FailReason, len(p.reasons))
	for path, reason := range p.reasons {
		reasons[path] = reason
	}
	p.mu.Unlock()
	query := NewFSIndexEngine(nil)
	for _, t := range targets {
		info := TargetFailInfo{Path: t.Path}
		counts := map[FailReason]uint{}
		for _, failure := range report.Failed {
			if failure.Path == "" || !atOrUnder(Norm(failure.Path), Norm(t.Path)) {
				continue
			}
			info.FailedLeaves++
			reason, ok := reasons[failure.Path]
			if !ok {
				reason = FailOther
			}
			counts[reason]++
			if counts[reason] > counts[info.Reason] {
				info.Reason = reason
			}
		}
		if info.FailedLeaves > 0 {
			report.FailInfo = append(report.FailInfo, info)
		}
		if p.Cancelled() || IsVirtualPath(t.Path) {
			continue
		}
		still := false
		for _, list := range [][]CleanFailure{report.Failed, report.SkippedItems, report.Manual} {
			for _, f := range list {
				if f.Path == t.Path || f.Path != "" && !t.RemoveDir && atOrUnder(Norm(f.Path), Norm(t.Path)) {
					still = true
				}
			}
		}
		if still {
			if m, ok := query.MeasurePath(context.Background(), t.Path); ok {
				report.RemainingLive = append(report.RemainingLive, RemainingLive{Path: t.Path, Bytes: m.Size, FileCount: m.Files})
			}
		}
	}
	snapshot := p.Snapshot()
	log.Printf("[删除] 完成：目标 ok=%d skipped=%d failed=%d；文件 %d 个 / %s，失败 %d 个", report.OK, report.Skipped, len(report.Failed), snapshot.Files, FmtSize(snapshot.Bytes), snapshot.Failed)
	return report
}

// launchAgentPlist reports a broken-login plist. The extension match is
// case-insensitive; `..` is not resolved, so a path cannot climb into the
// directory. home may be empty when the original user directory is unknown.
func launchAgentPlist(path, home string) bool {
	slash := strings.TrimRight(strings.ReplaceAll(path, `\`, "/"), "/")
	cut := strings.LastIndex(slash, "/")
	if cut <= 0 {
		return false
	}
	base := slash[cut+1:]
	dot := strings.LastIndex(base, ".")
	if dot <= 0 || !strings.EqualFold(base[dot:], ".plist") {
		return false
	}
	parent := slash[:cut]
	if parent == "/Library/LaunchAgents" {
		return true
	}
	if home == "" {
		return false
	}
	root := strings.TrimRight(strings.ReplaceAll(home, `\`, "/"), "/")
	return parent == root+"/Library/LaunchAgents"
}
