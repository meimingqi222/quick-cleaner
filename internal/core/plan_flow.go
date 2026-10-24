package core

import (
	"errors"
	"fmt"
)

type StepStatus string

const (
	StepSucceeded StepStatus = "succeeded"
	StepFailed    StepStatus = "failed"
	StepBlocked   StepStatus = "blocked"
	StepUnknown   StepStatus = "unknown"
	StepCancelled StepStatus = "cancelled"
)

type StepResult struct {
	Step   PlanStep   `json:"step"`
	Status StepStatus `json:"status"`
	Reason string     `json:"reason,omitempty"`
}
type PlanExecution struct {
	RuleID      string       `json:"rule_id"`
	RuleVersion uint32       `json:"rule_version"`
	Schema      uint32       `json:"schema"`
	Sequence    uint64       `json:"sequence"`
	Target      int          `json:"target"`
	Steps       []StepResult `json:"steps"`
}

// Only capability-owned executors can supply these methods. A rule declaration
// describes a target, never an executable callback or dependency ordering.
type planExecutor interface {
	supports(PlannedTarget) bool
	requiresOccupancy() bool
	prepare(PlannedTarget) (string, error)
	effective(PlanAction, PlannedTarget) PlanAction
	apply(PlannedTarget) CleanReport
	verify(CompletionCondition) Evidence
	failureReason() string
	unregister(PlannedTarget, string) error
	source(SourceAction) error
}

func executeCapability(plan *CleanupPlan, index int, p *CleanProgress, occupancy SpotCheck, capability planExecutor) CleanReport {
	var report CleanReport
	if index < 0 || index >= len(plan.targets) {
		report.RecordTarget(CleanFailure{ID: fmt.Sprintf("plan:%s:target:%d", plan.rule.id, index)}, CleanFailed)
		return report
	}
	target := plan.targets[index]
	o := plan.rule.observation
	execution := PlanExecution{RuleID: plan.rule.id, RuleVersion: o.RuleVersion, Schema: o.Schema, Sequence: o.Sequence, Target: index}
	dependencyFailed := false
	for _, step := range plan.Steps() {
		if step.Target != index {
			continue
		}
		step.Action = capability.effective(step.Action, target)
		status, reason := StepSucceeded, ""
		switch {
		case dependencyFailed:
			status, reason = StepBlocked, "Dependency did not succeed"
		case p.Cancelled():
			status, reason = StepCancelled, "Cancelled before step"
		default:
			switch step.Action.Kind {
			case "revalidate":
				supported := capability.supports(target) && (target.Operation.Kind == OpOfficialUninstall || target.Operation.ValidateTarget(target.Path, target.Operation.Kind != OpContents))
				if capability.requiresOccupancy() && occupancy != SpotClear {
					status, reason = StepBlocked, "Target occupancy is busy or unconfirmed"
					if occupancy == SpotUnknown {
						status = StepUnknown
					}
				} else if !supported {
					status, reason = StepBlocked, "Operation requires its native capability executor"
				} else if err := plan.Validate(); err != nil {
					status, reason = StepBlocked, err.Error()
				} else if note, err := capability.prepare(target); err != nil {
					status, reason = StepBlocked, err.Error()
				} else {
					reason = note
				}
			case "apply":
				report = capability.apply(target)
				if len(report.Failed) > 0 {
					status, reason = StepFailed, capability.failureReason()
					if reason == "" {
						reason = "Cleanup reported a failure"
					}
				} else if len(report.SkippedItems) > 0 || len(report.Manual) > 0 {
					status, reason = StepBlocked, "Cleanup skipped or requires manual action"
				}
			case "verify":
				if step.Action.Condition == nil {
					status, reason = StepUnknown, "Missing completion condition"
					break
				}
				switch capability.verify(*step.Action.Condition) {
				case Confirmed:
				case Absent:
					status, reason = StepFailed, "Cleanup completion condition is not met"
				default:
					status, reason = StepUnknown, "Cleanup completion could not be confirmed"
				}
			case "unregister_worktree":
				if err := capability.unregister(target, step.Action.Registration); err != nil {
					status, reason = StepFailed, err.Error()
				}
			case "source_lifecycle":
				var validation error
				if step.Action.Action == "revalidate" {
					if !capability.supports(target) {
						validation = errors.New("Unsupported source operation")
					} else if plan.installation == nil {
						validation = errors.New("Missing captured installation instance")
					} else {
						validation = plan.Validate()
					}
				}
				if validation != nil {
					status, reason = StepBlocked, validation.Error()
				} else if err := capability.source(step.Action.Action); err != nil {
					status, reason = StepFailed, err.Error()
				}
			default:
				status, reason = StepBlocked, "Unknown capability action"
			}
		}
		dependencyFailed = dependencyFailed || status != StepSucceeded
		execution.Steps = append(execution.Steps, StepResult{Step: step, Status: status, Reason: reason})
	}
	if dependencyFailed && len(report.Failed) == 0 && len(report.SkippedItems) == 0 && len(report.Manual) == 0 {
		report.OK = 0
		if p.Cancelled() {
			report.Record(target.Path, CleanSkipped)
		} else {
			p.mu.Lock()
			reason, exists := p.reasons[target.Path]
			p.mu.Unlock()
			if !exists {
				reason = FailUnverified
			}
			report.Record(target.Path, p.fail(target.Path, reason))
		}
	}
	report.PlanExecutions = append(report.PlanExecutions, execution)
	return report
}

type filesystemExecutor struct{ progress *CleanProgress }

func (e filesystemExecutor) supports(t PlannedTarget) bool {
	return (t.Operation.Kind == OpFile || t.Operation.Kind == OpTree || t.Operation.Kind == OpContents) && (t.Operation.Kind != OpContents || t.Disposal == Permanent)
}
func (filesystemExecutor) requiresOccupancy() bool                            { return true }
func (filesystemExecutor) prepare(PlannedTarget) (string, error)              { return "", nil }
func (filesystemExecutor) effective(a PlanAction, _ PlannedTarget) PlanAction { return a }
func (e filesystemExecutor) apply(t PlannedTarget) CleanReport {
	if t.Operation.Kind == OpContents {
		return CleanDirContents(t.Path, e.progress)
	}
	var report CleanReport
	report.Record(t.Path, Dispose(t.Path, t.Disposal, e.progress))
	return report
}
func (filesystemExecutor) verify(c CompletionCondition) Evidence { return FilesystemCompletion(c) }
func (filesystemExecutor) failureReason() string                 { return "" }
func (filesystemExecutor) unregister(PlannedTarget, string) error {
	return errors.New("Step requires its worktree capability executor")
}
func (filesystemExecutor) source(SourceAction) error {
	return errors.New("Step requires its source capability executor")
}

func ExecuteFilesystemPlan(plan *CleanupPlan, index int, p *CleanProgress, occupancy SpotCheck) CleanReport {
	return executeCapability(plan, index, p, occupancy, filesystemExecutor{progress: p})
}

type ownerExecutor struct {
	filesystemExecutor
	owner    *preparedOwner
	fallback bool
	sizeHint uint64
	error    string
}

func (e *ownerExecutor) supports(t PlannedTarget) bool {
	return (t.Operation.Kind == OpGo || t.Operation.Kind == OpPnpm) && t.Disposal == Permanent
}
func (e *ownerExecutor) prepare(t PlannedTarget) (string, error) {
	e.owner = prepareOwner(t.Path, t.Operation)
	e.fallback = e.owner == nil
	if e.fallback {
		return "Owner preflight unavailable or scope mismatched; using authorized contents cleanup", nil
	}
	return "", nil
}
func (e *ownerExecutor) effective(a PlanAction, t PlannedTarget) PlanAction {
	if !e.fallback {
		return a
	}
	if a.Kind == "apply" {
		op := Operation{Kind: OpContents}
		return PlanAction{Kind: "apply", Operation: &op}
	}
	if a.Kind == "verify" {
		condition := CompletionCondition{Kind: "contents_empty", Path: t.Path}
		return PlanAction{Kind: "verify", Condition: &condition}
	}
	return a
}
func (e *ownerExecutor) apply(t PlannedTarget) CleanReport {
	var report CleanReport
	if !RootIdentityHolds(t.Path, t.Identity) || IsProtected(t.Path) {
		e.error = "Target identity or protection changed during owner preflight"
		report.Record(t.Path, e.progress.fail(t.Path, FailChanged))
		return report
	}
	if e.fallback {
		return CleanDirContents(t.Path, e.progress)
	}
	if err := e.owner.apply(); err != nil {
		e.error = err.Error()
		report.Record(t.Path, e.progress.fail(t.Path, FailOther))
	} else {
		report.Record(t.Path, CleanOK)
	}
	return report
}
func (e *ownerExecutor) verify(c CompletionCondition) Evidence {
	if e.fallback {
		return FilesystemCompletion(c)
	}
	evidence := e.owner.completion()
	if evidence == Confirmed {
		e.progress.Files.Add(1)
		e.progress.Bytes.Add(e.sizeHint)
	}
	return evidence
}
func (e *ownerExecutor) failureReason() string { return e.error }
func ExecuteOwnerPlan(plan *CleanupPlan, index int, p *CleanProgress, occupancy SpotCheck, sizeHint uint64) CleanReport {
	return executeCapability(plan, index, p, occupancy, &ownerExecutor{filesystemExecutor: filesystemExecutor{progress: p}, sizeHint: sizeHint})
}

type nativeExecutor struct {
	filesystemExecutor
	sizeHint      uint64
	layersDeleted bool
	error         string
}

func (*nativeExecutor) supports(t PlannedTarget) bool {
	return (t.Operation.Kind == OpDocker || t.Operation.Kind == OpBrew || t.Operation.Kind == OpSnapshot) && t.Disposal == Permanent
}
func (*nativeExecutor) requiresOccupancy() bool { return false }
func (e *nativeExecutor) apply(t PlannedTarget) CleanReport {
	var report CleanReport
	var err error
	switch t.Operation.Kind {
	case OpDocker:
		e.layersDeleted, err = removeDockerImageAction(t.Operation.Reference)
	case OpBrew:
		err = brewCleanupAction()
	case OpSnapshot:
		if !removeSnapshotAction(t.Operation.Name) {
			err = errors.New("Native operation failed or unavailable")
		}
	default:
		err = errors.New("Native operation failed or unavailable")
	}
	if err != nil {
		e.error = err.Error()
		report.Record(t.Path, e.progress.fail(t.Path, FailOther))
	} else {
		report.Record(t.Path, CleanOK)
	}
	return report
}
func (e *nativeExecutor) verify(c CompletionCondition) Evidence {
	evidence := Unknown
	switch c.Kind {
	case "docker_reference_absent":
		evidence = dockerReferenceAbsence(c.Reference)
	case "snapshot_absent":
		evidence = snapshotAbsence(c.Name)
	case "brew_preview_empty":
		evidence = brewCleanupCompletion()
	}
	if evidence == Confirmed {
		if c.Kind == "brew_preview_empty" {
			recordBrewCleanup()
		}
		e.progress.Files.Add(1)
		if c.Kind == "brew_preview_empty" || c.Kind == "docker_reference_absent" && e.layersDeleted {
			e.progress.Bytes.Add(e.sizeHint)
		}
	}
	return evidence
}
func (e *nativeExecutor) failureReason() string { return e.error }

func ExecuteNativePlan(plan *CleanupPlan, index int, p *CleanProgress, sizeHint uint64) CleanReport {
	return executeCapability(plan, index, p, SpotUnknown, &nativeExecutor{filesystemExecutor: filesystemExecutor{progress: p}, sizeHint: sizeHint})
}

type registeredExecutor struct {
	filesystemExecutor
	run        func() error
	completion func(CompletionCondition) Evidence
	error      string
}

func (*registeredExecutor) supports(t PlannedTarget) bool {
	return t.Operation.Kind == OpOfficialUninstall && t.Disposal == Permanent
}
func (*registeredExecutor) requiresOccupancy() bool { return false }
func (e *registeredExecutor) apply(t PlannedTarget) CleanReport {
	var report CleanReport
	if err := e.run(); err != nil {
		e.error = err.Error()
		report.Record(t.Path, e.progress.fail(t.Path, FailOther))
	} else {
		report.Record(t.Path, CleanOK)
	}
	return report
}
func (e *registeredExecutor) verify(c CompletionCondition) Evidence { return e.completion(c) }
func (e *registeredExecutor) failureReason() string                 { return e.error }

func ExecuteRegisteredPlan(plan *CleanupPlan, index int, p *CleanProgress, run func() error, verify func(CompletionCondition) Evidence) CleanReport {
	return executeCapability(plan, index, p, SpotUnknown, &registeredExecutor{filesystemExecutor: filesystemExecutor{progress: p}, run: run, completion: verify})
}

type sourceExecutor struct {
	filesystemExecutor
	run       func(SourceAction) error
	lifecycle bool
}

func (e *sourceExecutor) supports(t PlannedTarget) bool {
	return e.lifecycle && t.Operation.Kind == OpOfficialUninstall && t.Disposal == Permanent
}
func (*sourceExecutor) requiresOccupancy() bool            { return false }
func (e *sourceExecutor) source(action SourceAction) error { return e.run(action) }

func ExecuteSourcePlan(plan *CleanupPlan, index int, p *CleanProgress, run func(SourceAction) error) CleanReport {
	definition := plan.rule.snapshot.Definition(plan.rule.id)
	return executeCapability(plan, index, p, SpotUnknown, &sourceExecutor{filesystemExecutor: filesystemExecutor{progress: p}, run: run, lifecycle: definition != nil && definition.App != nil})
}

func ExecutionResult(report CleanReport) error {
	steps := 0
	for _, execution := range report.PlanExecutions {
		for _, step := range execution.Steps {
			steps++
			if step.Status != StepSucceeded {
				return fmt.Errorf("Capability step %s: %s", step.Status, step.Reason)
			}
		}
	}
	if steps == 0 {
		return errors.New("No capability execution was reported")
	}
	if len(report.Failed) > 0 || len(report.SkippedItems) > 0 || len(report.Manual) > 0 {
		return errors.New("Capability reported incomplete cleanup")
	}
	return nil
}
