package core

import (
	"path/filepath"
	"reflect"
	"testing"
)

func TestDeclarationClaimsKeepProviderSiblingsVisible(t *testing.T) {
	home := t.TempDir()
	rule := RuleDefinition{ID: "fixture", Platform: "macos",
		Entries:  []PathRule{{Root: "home", Path: "Claimed/Dir"}, {Root: "home", Path: "{variable}/templated"}, {Root: "home", Path: "Deep/A/B/C"}},
		Catalogs: map[string][]Layout{"roots": {{Path: "Browser"}}, "agents": {{Path: "Agent", Children: []LayoutChild{{Path: "Cache"}, {Path: "Logs"}}}}},
		Directories: []DirectoryRule{
			{Root: "home", Paths: []string{"{roots}"}, Select: Selection{Kind: "signature_leaves"}},
			{Root: "home", Paths: []string{"{agents}"}, Select: Selection{Kind: "layout_children", Catalog: "agents"}},
			{Root: "home", Paths: []string{"Workspaces"}, Select: Selection{Kind: "linked_worktrees"}},
			{Root: "home", Paths: []string{"Unclaimed"}, Select: Selection{Kind: "children"}},
		},
	}
	s := &RuleSnapshot{Bundle: RuleBundle{Rules: []RuleDefinition{rule}}}
	c := CollectRuleClaims(s, map[string]string{"home": home}, "macos")
	for _, name := range []string{"Claimed", "Agent", "Workspaces", "Unclaimed", "Deep", "{variable}"} {
		if c.Owns(home, name) {
			t.Fatalf("provider root incorrectly hidden: %s", name)
		}
	}
	if !c.Owns(home, "Browser") || !c.Owns(filepath.Join(home, "Claimed"), "Dir") || !c.Owns(filepath.Join(home, "Agent"), "Cache") {
		t.Fatal("emitting declarations were not claimed")
	}
	if got := c.ClaimedChildren(home, "Agent"); !reflect.DeepEqual(got, []string{"Cache", "Logs"}) {
		t.Fatalf("direct children: %v", got)
	}
	if got := c.ClaimedChildren(home, "Deep"); len(got) != 0 {
		t.Fatalf("deep claim became shallow: %v", got)
	}
	if got := c.ClaimedChildren(filepath.Join(home, "Deep", "A"), "B"); !reflect.DeepEqual(got, []string{"C"}) {
		t.Fatalf("deep direct claim lost: %v", got)
	}
	for _, unavailable := range []RuleClaims{CollectRuleClaims(s, map[string]string{"home": home}, "windows"), CollectRuleClaims(s, nil, "macos")} {
		if unavailable.Owns(home, "Browser") || len(unavailable.ClaimedChildren(home, "Agent")) != 0 {
			t.Fatal("other platform or unavailable root claimed paths")
		}
	}
}
