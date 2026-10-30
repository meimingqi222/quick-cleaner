package core

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"testing"
)

func TestFilesystemTargetsShareFrozenRulesAndKeepCaptureIdentity(t *testing.T) {
	bundled, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	source := clonePlanValue(*bundled)
	for i := range source.Bundle.Rules {
		rule := &source.Bundle.Rules[i]
		rule.Entries, rule.Directories, rule.VersionLayouts = nil, nil, nil
	}
	source.Definition("engine").Lists["providers"] = []string{"cache"}
	home := t.TempDir()
	for i := range 24 {
		dir := filepath.Join(home, "Library", "Caches", fmt.Sprintf("fixture-cache-%d", i))
		if err := os.MkdirAll(dir, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "payload"), []byte("data"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	roots := map[string]string{"home": home}
	targets := FilesystemScanTargets(&source, roots, "macos")
	if len(targets) != 24 {
		t.Fatalf("got %d fixture targets", len(targets))
	}
	frozen := targets[0].Rule.snapshot
	if frozen == nil || frozen == &source {
		t.Fatal("source rules were not captured")
	}
	direct := ProviderRuleRef(&source, "engine", "cache_candidate")
	for _, target := range targets {
		if target.Rule.snapshot != frozen {
			t.Error("fixed target copied the rule bundle again")
		}
		if target.Rule.origin != &source || !reflect.DeepEqual(target.Rule, direct) {
			t.Error("shared capture changed rule identity or duplicate coalescing")
		}
	}
	before := targets[0].Rule.Observation()
	source.Bundle.Sequence++
	source.Definition("engine").Version++
	exported := targets[0].Rule.Observation()
	exported.ProviderPolicy.Recommended = !exported.ProviderPolicy.Recommended
	for _, target := range targets {
		got := target.Rule.Observation()
		if !reflect.DeepEqual(got, before) || target.Rule.snapshot.Bundle.Sequence != before.Sequence || target.Rule.snapshot.Definition("engine").Version != before.RuleVersion {
			t.Fatal("source or diagnostic mutation changed a captured rule")
		}
	}
	cats := ScanFixed(context.Background(), targets[:2], nil)
	var items []ScanItem
	for _, cat := range cats {
		items = append(items, cat.Items...)
	}
	if len(items) != 2 || len(items[0].Plans) != 1 || len(items[1].Plans) != 1 || items[0].Plans[0] == items[1].Plans[0] {
		t.Fatal("fixed targets shared or lost their independent plans")
	}
	for _, item := range items {
		if err := item.Plans[0].Validate(); err != nil {
			t.Fatal(err)
		}
		planned := item.Plans[0].Targets()
		if len(planned) != 1 || planned[0].Path != item.Path || planned[0].Identity == nil || !planned[0].Identity.Recheck(item.Path) {
			t.Fatal("fixed plan lost its own captured identity")
		}
	}
	next := FilesystemScanTargets(&source, roots, "macos")
	if len(next) != 24 || next[0].Rule.snapshot == frozen || next[0].Rule.Observation().Sequence != source.Bundle.Sequence {
		t.Fatal("next scan reused old mutable rules")
	}
	// Independent public captures must retain their original isolation contract.
	a, b := NewRuleRef(&source, "engine", ""), NewRuleRef(&source, "engine", "")
	if a.snapshot == b.snapshot || a.snapshot == &source {
		t.Fatal("ordinary NewRuleRef stopped freezing its input")
	}
}

func TestTargetNormalizationDoesNotAllocatePerTargetPair(t *testing.T) {
	targets := make([]ScanTarget, 512)
	for i := range targets {
		targets[i] = ScanTarget{Path: fmt.Sprintf("/Fixture/Cache/Target-%04d", i), Operation: Operation{Kind: OpContents}, Disposal: Permanent}
	}
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	normalized := NormalizeScanTargets(targets)
	runtime.ReadMemStats(&after)
	if len(normalized) != len(targets) {
		t.Fatal("normalization changed unrelated sibling targets")
	}
	if allocated := after.TotalAlloc - before.TotalAlloc; allocated > 2<<20 {
		t.Errorf("normalization allocated %d bytes across target pairs", allocated)
	}
	for i, target := range normalized {
		if target.Path != targets[i].Path {
			t.Fatal("normalization rewrote display or authority paths")
		}
	}
}

func TestCapturedRuleScopesKeepIndependentEvidence(t *testing.T) {
	source := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{{ID: "scope-fixture", Version: 1, Facts: map[string]Probe{"marker": {Kind: "file", Path: "marker"}}, Detect: &Condition{Kind: "fact", Name: "marker"}}}}}
	present, absent := t.TempDir(), t.TempDir()
	if err := os.WriteFile(filepath.Join(present, "marker"), []byte("evidence"), 0600); err != nil {
		t.Fatal(err)
	}
	captured := captureRuleSnapshot(source)
	a, b := NewRuleRef(captured, "scope-fixture", present), NewRuleRef(captured, "scope-fixture", absent)
	if a.snapshot != b.snapshot || a.origin != source || b.origin != source {
		t.Fatal("scope capture did not share the frozen bundle and original identity")
	}
	if a.Observation().Detected != Confirmed || b.Observation().Detected != Absent || a.Observation().Scope != present || b.Observation().Scope != absent {
		t.Fatal("different scopes shared evidence")
	}
	a.observation.Facts["marker"] = Unknown
	if b.Observation().Facts["marker"] != Absent {
		t.Fatal("scope fact maps were shared")
	}
	source.Definition("scope-fixture").Facts["marker"] = Probe{Kind: "file", Path: "replacement"}
	if NewRuleRef(captured, "scope-fixture", present).Observation().Detected != Confirmed {
		t.Fatal("source mutation changed captured scope evidence")
	}
	next := NewRuleRef(captureRuleSnapshot(source), "scope-fixture", present)
	if next.Observation().Detected != Absent {
		t.Fatal("next capture ignored source changes")
	}
}
