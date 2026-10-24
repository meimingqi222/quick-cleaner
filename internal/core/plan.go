package core

import (
	"errors"
	"fmt"
	"os"
)

type PlannedTarget struct {
	Path      string
	Operation Operation
	Identity  *TargetIdentity
	Disposal  Disposal
}

type TargetScope struct {
	Kind         string       `json:"kind"`
	Path         string       `json:"path,omitempty"`
	Checkout     string       `json:"checkout,omitempty"`
	Registration string       `json:"registration,omitempty"`
	Key          string       `json:"key,omitempty"`
	Identifier   string       `json:"identifier,omitempty"`
	Root         string       `json:"root,omitempty"`
	Extent       *TargetScope `json:"extent,omitempty"`
}

type CompletionCondition struct {
	Kind         string `json:"kind"`
	Path         string `json:"path,omitempty"`
	Reference    string `json:"reference,omitempty"`
	Name         string `json:"name,omitempty"`
	Root         string `json:"root,omitempty"`
	Checkout     string `json:"checkout,omitempty"`
	Registration string `json:"registration,omitempty"`
	Identifier   string `json:"identifier,omitempty"`
}

func (t PlannedTarget) Completion() CompletionCondition {
	c := CompletionCondition{Path: t.Path}
	switch t.Operation.Kind {
	case OpFile, OpTree:
		c.Kind = "path_absent"
	case OpContents:
		c.Kind = "contents_empty"
	case OpDocker:
		c = CompletionCondition{Kind: "docker_reference_absent", Reference: t.Operation.Reference}
	case OpSnapshot:
		c = CompletionCondition{Kind: "snapshot_absent", Name: t.Operation.Name}
	case OpBrew:
		c = CompletionCondition{Kind: "brew_preview_empty"}
	case OpGo:
		c.Kind = "go_cache_absent"
	case OpPnpm:
		c.Kind = "pnpm_store_healthy"
	case OpTrash:
		c = CompletionCondition{Kind: "trash_inventory_empty", Root: t.Path}
	case OpGitWorktree:
		c = CompletionCondition{Kind: "worktree_and_registration_absent", Checkout: t.Path, Registration: t.Operation.Registration}
	case OpNative, OpRegistration:
		c = CompletionCondition{Kind: "registration_absent", Identifier: t.Path}
	case OpOfficialUninstall:
		c = CompletionCondition{Kind: "installation_artifacts_and_registrations_absent", Root: t.Path}
	}
	return c
}

func (t PlannedTarget) scope() TargetScope {
	s := TargetScope{Path: t.Path}
	switch t.Operation.Kind {
	case OpFile:
		s.Kind = "file"
	case OpTree:
		s.Kind = "owned_tree"
	case OpContents, OpGo, OpPnpm, OpTrash:
		s.Kind = "contents"
	case OpGitWorktree:
		s = TargetScope{Kind: "worktree", Checkout: t.Path, Registration: t.Operation.Registration}
	case OpDocker, OpSnapshot, OpBrew, OpNative:
		s = TargetScope{Kind: "native_resource", Key: t.Operation.TargetKey(t.Path)}
	case OpRegistration:
		s = TargetScope{Kind: "registration", Identifier: t.Path}
	case OpOfficialUninstall:
		s = TargetScope{Kind: "installation", Root: t.Path}
	}
	return s
}

type PlanAction struct {
	Kind         string               `json:"kind"`
	Operation    *Operation           `json:"operation,omitempty"`
	Condition    *CompletionCondition `json:"condition,omitempty"`
	Action       SourceAction         `json:"action,omitempty"`
	Registration string               `json:"registration,omitempty"`
}
type PlanStep struct {
	ID        int        `json:"id"`
	Target    int        `json:"target"`
	DependsOn []int      `json:"depends_on"`
	Action    PlanAction `json:"action"`
}

// CleanupPlan keeps execution authority private. Explanations and returned
// targets are copies, so diagnostic serialization cannot create a new plan.
type CleanupPlan struct {
	rule         RuleRef
	targets      []PlannedTarget
	preserve     []string
	blocked      []string
	installation *InstallationInstance
	official     *OfficialOperation
}

func NewCleanupPlan(rule RuleRef, targets []PlannedTarget) *CleanupPlan {
	p := &CleanupPlan{rule: rule, preserve: rule.Preserved()}
	p.targets = make([]PlannedTarget, len(targets))
	for i, t := range targets {
		if t.Identity != nil {
			id := *t.Identity
			t.Identity = &id
		}
		p.targets[i] = t
	}
	for _, o := range rule.Observations() {
		if o.Detected != Confirmed {
			p.blocked = append(p.blocked, "Scan ownership evidence absent or unknown: "+o.RuleID)
		}
	}
	return p
}

func (p *CleanupPlan) RuleID() string {
	if p == nil {
		return ""
	}
	return p.rule.ID()
}

func (p *CleanupPlan) Blocked() []string {
	if p == nil {
		return nil
	}
	return append([]string(nil), p.blocked...)
}

func (p *CleanupPlan) Targets() []PlannedTarget {
	copy := make([]PlannedTarget, len(p.targets))
	for i, t := range p.targets {
		if t.Identity != nil {
			id := *t.Identity
			t.Identity = &id
		}
		copy[i] = t
	}
	return copy
}

func (p *CleanupPlan) Scope(index int) (TargetScope, bool) {
	if index < 0 || index >= len(p.targets) {
		return TargetScope{}, false
	}
	t := p.targets[index]
	s := t.scope()
	if p.rule.manifestMembership() && !t.Operation.IsNativeResource() {
		s = TargetScope{Kind: "manifest_member", Path: t.Path, Extent: &s}
	}
	return s, true
}

func (p *CleanupPlan) Steps() []PlanStep {
	var steps []PlanStep
	for index, t := range p.targets {
		op := t.Operation
		condition := t.Completion()
		actions := []PlanAction{{Kind: "revalidate"}, {Kind: "apply", Operation: &op}}
		if op.Kind == OpGitWorktree {
			absent := CompletionCondition{Kind: "path_absent", Path: t.Path}
			actions = append(actions, PlanAction{Kind: "verify", Condition: &absent}, PlanAction{Kind: "unregister_worktree", Registration: op.Registration})
		}
		actions = append(actions, PlanAction{Kind: "verify", Condition: &condition})
		if op.Kind == OpOfficialUninstall && p.rule.snapshot != nil {
			if d := p.rule.snapshot.Definition(p.rule.id); d != nil && d.App != nil {
				actions = nil
				for _, a := range []SourceAction{"revalidate", "official_operation", "supplement_cleanup", "verify_completion", "remove_recovery_records"} {
					actions = append(actions, PlanAction{Kind: "source_lifecycle", Action: a})
				}
			}
		}
		first := len(steps)
		for _, a := range actions {
			id := len(steps)
			deps := []int{}
			if id != first {
				deps = append(deps, id-1)
			}
			steps = append(steps, PlanStep{ID: id, Target: index, DependsOn: deps, Action: a})
		}
	}
	return steps
}

func (p *CleanupPlan) ValidateBinding(path string, operation Operation, disposal Disposal, identity *TargetIdentity) error {
	for _, t := range p.targets {
		idsMatch := t.Identity == nil && identity == nil || t.Identity != nil && identity != nil && *t.Identity == *identity
		if t.Operation.TargetKey(t.Path) == operation.TargetKey(path) && t.Operation == operation && t.Disposal == disposal && idsMatch {
			return p.Validate()
		}
	}
	return errors.New("Selection does not match the scanned plan")
}

func (p *CleanupPlan) Validate() error {
	if err := p.rule.Revalidate(); err != nil {
		return err
	}
	if len(p.blocked) > 0 {
		return errors.New(p.blocked[0])
	}
	if p.installation != nil {
		if err := p.installation.Revalidate(); err != nil {
			return err
		}
	}
	if p.official != nil {
		if _, err := p.official.Command(); err != nil {
			return err
		}
	}
	preserve := append(append([]string(nil), p.preserve...), p.rule.PreservedLive()...)
	for i, t := range p.targets {
		if err := p.rule.Authorizes(t.Path); err != nil {
			return err
		}
		if t.Operation.IsFilesystemBacked() {
			protected := IsProtected(t.Path)
			if t.Operation.Kind == OpContents {
				protected = IsContentsProtected(t.Path)
			}
			if protected {
				return errors.New("Protected target")
			}
			if t.Identity == nil {
				return errors.New("Missing target identity")
			}
			if !t.Identity.Recheck(t.Path) {
				return errors.New("Target identity changed or unavailable")
			}
			n := Norm(t.Path)
			for _, keep := range preserve {
				if atOrUnder(n, Norm(keep)) {
					return fmt.Errorf("Target is inside a preserved path: %s", keep)
				}
			}
			for _, keep := range preserve {
				if atOrUnder(Norm(keep), n) {
					return fmt.Errorf("Target covers a preserved path: %s", keep)
				}
			}
		}
		for _, other := range p.targets[:i] {
			if Norm(t.Path) == Norm(other.Path) && t.Operation != other.Operation {
				return errors.New("Conflicting target operations")
			}
		}
	}
	return nil
}

func (p *CleanupPlan) Explanation() map[string]any {
	targets := make([]map[string]any, len(p.targets))
	for i, t := range p.targets {
		scope, _ := p.Scope(i)
		targets[i] = map[string]any{"path": t.Path, "operation": t.Operation, "disposal": t.Disposal, "identity_captured": t.Identity != nil, "scope": scope, "completion": t.Completion()}
	}
	out := map[string]any{"observations": p.rule.Observations(), "targets": targets, "steps": p.Steps(), "preserve": append([]string(nil), p.preserve...), "blocked": append([]string(nil), p.blocked...)}
	if p.installation != nil {
		out["installation"] = p.installation.Explanation()
	}
	if p.official != nil {
		out["official"] = p.official.Explanation()
	}
	return out
}

// FilesystemCompletion requires positive evidence. An unreadable path is not
// absent, and an unreadable directory is not empty.
func FilesystemCompletion(condition CompletionCondition) Evidence {
	switch condition.Kind {
	case "path_absent":
		_, err := os.Lstat(condition.Path)
		if os.IsNotExist(err) {
			return Confirmed
		}
		if err != nil {
			return Unknown
		}
		return Absent
	case "contents_empty":
		md, err := os.Lstat(condition.Path)
		if err != nil || ruleEvidenceLink(md) || !md.IsDir() {
			return Unknown
		}
		if _, ok := CaptureIdentity(condition.Path); !ok {
			return Unknown
		}
		entries, err := os.ReadDir(condition.Path)
		if err != nil {
			return Unknown
		}
		if len(entries) == 0 {
			return Confirmed
		}
		return Absent
	}
	return Unknown
}
