package core

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"testing/fstest"

	bundled "github.com/meimingqi222/quick-cleaner/rules"
)

func TestRuleBundledSourceContract(t *testing.T) {
	s, e := CurrentRules()
	if e != nil {
		t.Fatal(e)
	}
	source, e := LoadRuleDirectory("../../rules", 1)
	if e != nil {
		t.Fatal(e)
	}
	if !reflect.DeepEqual(source, s.Bundle) {
		t.Fatal("embedded declarations differ from source")
	}
	if len(s.Bundle.Rules) != 14 {
		t.Fatalf("rules=%d", len(s.Bundle.Rules))
	}
	if s.Definition("missing") != nil || RuleNumber("missing", "x") != 0 || len(RuleList("missing", "x")) != 0 {
		t.Fatal("unsafe missing lookup")
	}
	data, e := os.ReadFile("../../rules/fixtures/provider-policy-baseline.json")
	if e != nil {
		t.Fatal(e)
	}
	var rows []struct {
		Policy      string
		Recommended bool
		Disposal    Disposal
	}
	if e = json.Unmarshal(data, &rows); e != nil {
		t.Fatal(e)
	}
	for _, row := range rows {
		p, ok := s.ProviderPolicy("engine", row.Policy)
		if !ok || p.Recommended != row.Recommended || p.Disposal != row.Disposal {
			t.Fatalf("provider drift %s", row.Policy)
		}
	}
	p, ok := s.CategoryPolicy("DevWorktrees")
	if !ok || p.Safety != "danger" || p.Group != "developer" {
		t.Fatal("category policy drift")
	}
	if RuleNumber("engine", "photo_distance") != 5 {
		t.Fatal("number drift")
	}
	list := RuleList("engine", "providers")
	list[0] = "mutated"
	if RuleList("engine", "providers")[0] == "mutated" {
		t.Fatal("list aliases snapshot")
	}
	for _, key := range []string{"satellite_dirs", "user_family_dirs", "system_family_dirs", "orphan_roots"} {
		if len(s.Definition("residual-macos").Locations[key]) == 0 {
			t.Fatal("missing residual anchor", key)
		}
	}
}
func ruleTestBundle(t *testing.T) RuleBundle {
	t.Helper()
	s, e := CurrentRules()
	if e != nil {
		t.Fatal(e)
	}
	data, e := json.Marshal(s.Bundle)
	if e != nil {
		t.Fatal(e)
	}
	b, e := ParseRuleBundle(data)
	if e != nil {
		t.Fatal(e)
	}
	return b
}
func TestRuleValidationRejectsUnsafeDeclarations(t *testing.T) {
	mutations := map[string]func(*RuleSnapshot){
		"capability": func(s *RuleSnapshot) {
			s.Definition("windows").Required = append(s.Definition("windows").Required, "shell")
		},
		"root":      func(s *RuleSnapshot) { s.Definition("windows").Entries[0].Root = "drive:../C" },
		"path":      func(s *RuleSnapshot) { s.Definition("windows").Entries[0].Path = "../outside" },
		"operation": func(s *RuleSnapshot) { s.Definition("windows").Entries[0].Operation = "shell" },
		"condition": func(s *RuleSnapshot) { s.Definition("windows").Detect = &Condition{Kind: "shell"} },
		"fact": func(s *RuleSnapshot) {
			s.Definition("windows").Facts = map[string]Probe{"x": {Kind: "shell", Path: "x"}}
		},
		"category":     func(s *RuleSnapshot) { delete(s.Definition("engine").Categories, "Logs") },
		"policy bound": func(s *RuleSnapshot) { s.Definition("engine").Numbers["photo_distance"] = 64 },
		"classification": func(s *RuleSnapshot) {
			r := s.Definition("engine")
			r.Lists["download_archive"] = append(r.Lists["download_archive"], "exe")
		},
		"source arguments": func(s *RuleSnapshot) {
			s.Definition("hermes").App.ModuleArgs = append(s.Definition("hermes").App.ModuleArgs, "--eval")
		},
		"source lifecycle": func(s *RuleSnapshot) { s.Definition("hermes").App.Actions = nil },
		"residual fences":  func(s *RuleSnapshot) { s.Definition("residual-macos").Lists["protected_dot_dirs"] = nil },
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			s := RuleSnapshot{Bundle: ruleTestBundle(t)}
			mutate(&s)
			if s.Bundle.Validate() == nil {
				t.Fatal("unsafe mutation accepted")
			}
		})
	}
	data, e := bundled.Files.ReadFile("windows.toml")
	if e != nil {
		t.Fatal(e)
	}
	_, e = LoadRuleBundle(fstest.MapFS{"bad.toml": {Data: append([]byte("unknown_safety_condition = true\n"), data...)}}, 1)
	if e == nil {
		t.Fatal("unknown field accepted")
	}
}
func TestRuleEvidenceAndUnknownContract(t *testing.T) {
	root := t.TempDir()
	if e := os.WriteFile(filepath.Join(root, "manifest.json"), []byte(`{"owner":"fixture","members":["cache","cache"],"version":"1.2"}`), 0600); e != nil {
		t.Fatal(e)
	}
	if e := os.Mkdir(filepath.Join(root, "cache"), 0700); e != nil {
		t.Fatal(e)
	}
	for _, tc := range []struct {
		p    Probe
		want Evidence
	}{
		{Probe{Kind: "json", Path: "manifest.json", Pointer: "/owner", Equals: "fixture"}, Confirmed},
		{Probe{Kind: "json", Path: "manifest.json", Pointer: "/owner", Equals: "other"}, Absent},
		{Probe{Kind: "file", Path: "missing"}, Absent},
		{Probe{Kind: "file", Path: "../manifest.json"}, Unknown},
		{Probe{Kind: "text", Path: "manifest.json", Signature: "fixture"}, Confirmed},
		{Probe{Kind: "file", Path: "cache"}, Absent},
		{Probe{Kind: "unknown", Path: "manifest.json"}, Unknown},
	} {
		if got := tc.p.Evaluate(root); got != tc.want {
			t.Fatalf("%+v: %s != %s", tc.p, got, tc.want)
		}
	}
	c := Condition{Kind: "not", Condition: &Condition{Kind: "fact", Name: "busy"}}
	if c.Evaluate(nil) != Unknown || (Condition{Kind: "all"}).Evaluate(nil) != Unknown {
		t.Fatal("unknown evidence authorized")
	}
	if c.Validate(nil) == nil {
		t.Fatal("unknown fact accepted")
	}
	f, e := os.Create(filepath.Join(root, "huge"))
	if e != nil {
		t.Fatal(e)
	}
	if e = f.Truncate(4*1024*1024 + 1); e != nil {
		t.Fatal(e)
	}
	f.Close()
	if (Probe{Kind: "text", Path: "huge", Signature: "x"}).Evaluate(root) != Unknown {
		t.Fatal("unbounded evidence")
	}
	if e := os.Symlink(root, filepath.Join(root, "redirect")); e == nil {
		if (Probe{Kind: "file", Path: "redirect/manifest.json"}).Evaluate(root) != Unknown {
			t.Fatal("redirected evidence authorized")
		}
		if ok, e := ConfinedRulePath(filepath.Join(root, "redirect"), filepath.Join(root, "redirect", "cache")); e == nil || ok {
			t.Fatal("redirected root accepted")
		}
	}
}
func TestRuleVariablesGraphAndBounds(t *testing.T) {
	root := t.TempDir()
	if e := os.MkdirAll(filepath.Join(root, "app", "cache"), 0700); e != nil {
		t.Fatal(e)
	}
	write := func(s string) {
		t.Helper()
		if e := os.WriteFile(filepath.Join(root, "app", "manifest.json"), []byte(s), 0600); e != nil {
			t.Fatal(e)
		}
	}
	write(`{"members":["cache","cache"],"version":"1.2"}`)
	vars := map[string]Variable{"app": {Kind: "path", Path: "app"}, "members": {Kind: "json_set", Path: "${app}/manifest.json", Pointer: "/members"}, "version": {Kind: "json_field", Path: "${app}/manifest.json", Pointer: "/version"}, "key": {Kind: "canonical_hash", Path: "${app}", Length: 16}, "dirs": {Kind: "directories", Path: "app", Limit: 8}}
	values := EvaluateRuleVariables(vars, root)
	paths, e := ExpandRulePaths("${app}/${members}", values)
	if e != nil || !reflect.DeepEqual(paths, []string{"app/cache"}) {
		t.Fatal(paths, e)
	}
	if values["version"].Value != "1.2" || len(values["key"].Value.(string)) != 16 {
		t.Fatal(values)
	}
	paths, e = ExpandRulePaths("${dirs}", values)
	if e != nil || !reflect.DeepEqual(paths, []string{"app/cache"}) {
		t.Fatal(paths, e)
	}
	write(`{"members":["../outside"]}`)
	if _, e := ExpandRulePaths("${app}/${members}", EvaluateRuleVariables(vars, root)); e == nil {
		t.Fatal("variable escape accepted")
	}
	cycle := map[string]Variable{"a": {Kind: "path", Path: "${b}"}, "b": {Kind: "path", Path: "${a}"}}
	if _, e := ValidateRuleVariables(cycle); e == nil {
		t.Fatal("cycle accepted")
	}
	chain := map[string]Variable{}
	for i := 0; i < 34; i++ {
		path := "app"
		if i < 33 {
			path = fmt.Sprintf("${v%02d}", i+1)
		}
		chain[fmt.Sprintf("v%02d", i)] = Variable{Kind: "path", Path: path}
	}
	if _, e := ValidateRuleVariables(chain); e == nil {
		t.Fatal("depth bound ignored")
	}
	vars["dirs"] = Variable{Kind: "directories", Path: "app", Limit: 1}
	if EvaluateRuleVariables(vars, root)["dirs"].Evidence != Unknown {
		t.Fatal("partial enumeration authorized")
	}
	if e := os.Mkdir(filepath.Join(root, "empty"), 0700); e != nil {
		t.Fatal(e)
	}
	if EvaluateRuleVariables(map[string]Variable{"empty": {Kind: "directories", Path: "empty", Limit: 1}}, root)["empty"].Evidence != Confirmed {
		t.Fatal("empty enumeration is not confirmed")
	}
	replacements := []string{}
	for i := 0; i < 257; i++ {
		replacements = append(replacements, fmt.Sprint(i))
	}
	if _, e := ExpandRulePaths("${x}", VariableValues{"x": {Evidence: Confirmed, Value: replacements}}); e == nil {
		t.Fatal("expansion bound ignored")
	}
	if _, e := ExpandRulePaths("${x}", VariableValues{"x": {Evidence: Unknown, Value: "safe"}}); e == nil {
		t.Fatal("unknown variable authorized")
	}
	if _, e := ExpandRulePaths(strings.Repeat("a", 1025), nil); e == nil {
		t.Fatal("path limit ignored")
	}
}
