package core

import (
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
)

type RuleObservation struct {
	RuleID         string              `json:"rule_id"`
	RuleVersion    uint32              `json:"rule_version"`
	Schema         uint32              `json:"schema"`
	Sequence       uint64              `json:"sequence"`
	Scope          string              `json:"scope"`
	Variables      VariableValues      `json:"variables"`
	Facts          map[string]Evidence `json:"facts"`
	Detected       Evidence            `json:"detected"`
	ProviderKey    string              `json:"provider_key,omitempty"`
	ProviderPolicy *ProviderPolicy     `json:"provider_policy,omitempty"`
	VersionGuard   *VersionGuard       `json:"version_guard,omitempty"`
}

// Private captured state cannot be reconstructed from a diagnostic JSON export.
type RuleRef struct {
	snapshot     *RuleSnapshot
	origin       *RuleSnapshot
	id, scope    string
	contributors []RuleRef
	blocked      string
	observation  RuleObservation
}

func clonePlanValue[T any](v T) T {
	var out T
	b, _ := json.Marshal(v)
	_ = json.Unmarshal(b, &out)
	return out
}

type ruleSnapshotCapture struct {
	origin, frozen *RuleSnapshot
}

// The enumeration wrapper is local to one scan. Returned refs hold only the
// plain frozen snapshot and original identity, preserving Merge semantics.
func captureRuleSnapshot(source *RuleSnapshot) *RuleSnapshot {
	if source == nil || source.capture != nil {
		return source
	}
	frozen := &RuleSnapshot{Bundle: clonePlanValue(source.Bundle)}
	return &RuleSnapshot{Bundle: frozen.Bundle, capture: &ruleSnapshotCapture{source, frozen}}
}

func NewRuleRef(snapshot *RuleSnapshot, id, scope string) RuleRef {
	r := RuleRef{origin: snapshot, id: id, scope: scope}
	if snapshot == nil {
		r.blocked = "Missing rule snapshot"
		return r
	}
	if capture := snapshot.capture; capture != nil {
		r.origin, r.snapshot = capture.origin, capture.frozen
	} else {
		r.snapshot = &RuleSnapshot{Bundle: clonePlanValue(snapshot.Bundle)}
	}
	d := r.snapshot.Definition(id)
	if d == nil {
		r.blocked = "Unknown rule: " + id
		return r
	}
	o := RuleObservation{RuleID: id, RuleVersion: d.Version, Schema: snapshot.Bundle.Schema, Sequence: snapshot.Bundle.Sequence, Scope: scope, Detected: Confirmed}
	if scope != "" {
		o.Variables, o.Facts = d.EvidenceAt(scope)
	}
	if d.Detect != nil {
		o.Detected = d.Detect.Evaluate(o.Facts)
	}
	r.observation = o
	return r
}
func EngineRuleRef() RuleRef { s, _ := CurrentRules(); return NewRuleRef(s, "engine", "") }
func BuildRuleRef() RuleRef  { s, _ := CurrentRules(); return NewRuleRef(s, "build", "") }
func ProviderRuleRef(snapshot *RuleSnapshot, id, key string) RuleRef {
	r := NewRuleRef(snapshot, id, "")
	if r.snapshot == nil {
		return r
	}
	p, ok := r.snapshot.ProviderPolicy(id, key)
	if !ok {
		r.blocked = "Missing provider policy: " + id + "/" + key
	} else {
		r.observation.ProviderKey = key
		r.observation.ProviderPolicy = &p
	}
	return r
}

// BlockedReason is the rule constraint that prevents cleanup. Empty means runnable.
func (r RuleRef) BlockedReason() string {
	if r.blocked != "" {
		return r.blocked
	}
	for _, c := range r.contributors {
		if reason := c.BlockedReason(); reason != "" {
			return reason
		}
	}
	return ""
}

func (r RuleRef) ID() string                   { return r.id }
func (r RuleRef) Observation() RuleObservation { return clonePlanValue(r.observation) }
func (r RuleRef) Merge(other RuleRef) RuleRef {
	if reflect.DeepEqual(r, other) {
		return r
	}
	for _, contributor := range r.contributors {
		if reflect.DeepEqual(contributor, other) {
			return r
		}
	}
	r.contributors = append(append([]RuleRef(nil), r.contributors...), other)
	if r.origin != other.origin {
		r.blocked = "Mixed rule snapshots"
	}
	return r
}
func (r RuleRef) Observations() []RuleObservation {
	out := []RuleObservation{r.Observation()}
	for _, c := range r.contributors {
		out = append(out, c.Observations()...)
	}
	return out
}
func (r RuleRef) DeclaresPreserve() bool {
	if r.snapshot != nil {
		if d := r.snapshot.Definition(r.id); d != nil && (len(d.Preserve) > 0 || d.App != nil && len(d.App.Preserve) > 0) {
			return true
		}
	}
	for _, c := range r.contributors {
		if c.DeclaresPreserve() {
			return true
		}
	}
	return false
}
func (r RuleRef) preserved(live bool) []string {
	var out []string
	for _, c := range r.contributors {
		out = append(out, c.preserved(live)...)
	}
	if r.scope == "" || r.snapshot == nil {
		return out
	}
	d := r.snapshot.Definition(r.id)
	if d == nil {
		return out
	}
	values := r.observation.Variables
	if live {
		values = EvaluateRuleVariables(d.Variables, r.scope)
	}
	templates := append([]string(nil), d.Preserve...)
	if d.App != nil {
		templates = append(templates, d.App.Preserve...)
	}
	for _, t := range templates {
		paths, e := ExpandRulePaths(t, values)
		if e != nil {
			out = append(out, r.scope)
			continue
		}
		for _, p := range paths {
			out = append(out, filepath.Join(r.scope, strings.ReplaceAll(p, `\`, "/")))
		}
	}
	return out
}
func (r RuleRef) Preserved() []string     { return r.preserved(false) }
func (r RuleRef) PreservedLive() []string { return r.preserved(true) }
func (r RuleRef) Revalidate() error {
	if r.blocked != "" {
		return errors.New(r.blocked)
	}
	if r.snapshot == nil {
		return errors.New("Missing snapshot")
	}
	d := r.snapshot.Definition(r.id)
	if d == nil {
		return errors.New("Unknown rule")
	}
	if r.observation.Detected != Confirmed {
		return errors.New("Scan ownership evidence absent or unknown")
	}
	for _, c := range r.contributors {
		if e := c.Revalidate(); e != nil {
			return e
		}
	}
	if guard := r.observation.VersionGuard; guard != nil {
		if err := guard.revalidate(d, r.scope); err != nil {
			return err
		}
	}
	if r.scope == "" && (len(d.Variables) > 0 || d.Detect != nil) {
		return errors.New("Missing evidence scope")
	}
	if d.Detect != nil && d.MatchesAt(r.scope) != Confirmed {
		return errors.New("Ownership evidence changed or unavailable")
	}
	values := EvaluateRuleVariables(d.Variables, r.scope)
	templates := append([]string(nil), d.Preserve...)
	if d.App != nil {
		templates = append(templates, d.App.Preserve...)
	}
	for _, t := range templates {
		if _, e := ExpandRulePaths(t, values); e != nil {
			return e
		}
	}
	return nil
}
func (r RuleRef) Authorizes(path string) error {
	if e := r.Revalidate(); e != nil {
		return e
	}
	for _, c := range r.contributors {
		if e := c.Authorizes(path); e != nil {
			return e
		}
	}
	d := r.snapshot.Definition(r.id)
	if len(d.Variables) == 0 {
		return nil
	}
	// Both observed and current membership must authorize; later additions cannot widen a scan.
	for _, values := range []VariableValues{r.observation.Variables, EvaluateRuleVariables(d.Variables, r.scope)} {
		ok := false
		for _, entry := range d.Entries {
			paths, e := ExpandRulePaths(entry.Path, values)
			if e != nil {
				continue
			}
			for _, p := range paths {
				if Norm(filepath.Join(r.scope, strings.ReplaceAll(p, `\`, "/"))) == Norm(path) {
					ok = true
				}
			}
		}
		if !ok {
			return errors.New("Manifest membership changed or unavailable")
		}
	}
	return nil
}
func (r RuleRef) manifestMembership() bool {
	if r.snapshot != nil {
		if d := r.snapshot.Definition(r.id); d != nil {
			for _, v := range d.Variables {
				if v.Kind == "json_set" {
					return true
				}
			}
		}
	}
	for _, c := range r.contributors {
		if c.manifestMembership() {
			return true
		}
	}
	return false
}
