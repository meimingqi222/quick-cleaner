package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestBuildDiscoverySharesFrozenRuleAndKeepsTargetsIndependent(t *testing.T) {
	root := buildProjectFixture(t)
	var entries []TreeSnapshotEntry
	for _, project := range []string{"alpha", "beta"} {
		dir := filepath.Join(root, project)
		target := filepath.Join(dir, "target")
		if err := os.MkdirAll(target, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "Cargo.toml"), []byte("[package]"), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(target, "payload"), []byte("build output"), 0600); err != nil {
			t.Fatal(err)
		}
		entries = append(entries,
			TreeSnapshotEntry{Path: dir, IsDir: true},
			TreeSnapshotEntry{Path: target, IsDir: true},
			TreeSnapshotEntry{Path: filepath.Join(dir, "Cargo.toml"), Size: 9},
			TreeSnapshotEntry{Path: filepath.Join(target, "payload"), Size: 12},
		)
	}
	tree := SizeTreeFromSnapshot(VolumeFromMountPoint(root), entries)
	bundled, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	for _, channel := range []struct {
		name string
		tree *SizeTree
	}{{"walk", nil}, {"index", tree}} {
		t.Run(channel.name, func(t *testing.T) {
			snapshot := clonePlanValue(*bundled)
			items := DiscoverBuildArtifacts(context.Background(), &snapshot, []CodeRoot{{root, 10}}, channel.tree)
			if len(items) != 2 {
				t.Fatalf("discovery returned %d targets, want 2", len(items))
			}
			frozen := items[0].Rule.snapshot
			if frozen == nil || frozen == &snapshot {
				t.Fatal("rule was not frozen away from the source")
			}
			if items[1].Rule.snapshot != frozen {
				t.Error("discovery copied the rule bundle per target instead of once per scan")
			}
			before := items[0].Rule.Observation()
			snapshot.Bundle.Sequence++
			snapshot.Definition("build").Version++
			for _, item := range items {
				if item.Rule.Observation().Sequence != before.Sequence || item.Rule.Observation().RuleVersion != before.RuleVersion {
					t.Error("source mutation changed a frozen observation")
				}
				if item.Rule.snapshot.Bundle.Sequence != before.Sequence || item.Rule.snapshot.Definition("build").Version != before.RuleVersion {
					t.Error("source mutation changed the frozen rule definition")
				}
				if len(item.Plans) != 1 || item.Plans[0].Validate() != nil {
					t.Fatal("discovery lost a validated target plan")
				}
				targets := item.Plans[0].Targets()
				if len(targets) != 1 || targets[0].Path != item.Path || targets[0].Identity == nil || !targets[0].Identity.Recheck(item.Path) {
					t.Fatal("plan lost its own target identity")
				}
				targets[0].Path = "changed diagnostic copy"
				*targets[0].Identity = TargetIdentity{}
				if again := item.Plans[0].Targets(); again[0].Path != item.Path || !again[0].Identity.Recheck(item.Path) {
					t.Fatal("diagnostic target copy changed the plan")
				}
			}
			if items[0].Plans[0] == items[1].Plans[0] || items[0].Identity == items[1].Identity {
				t.Fatal("targets shared execution state")
			}
			next := DiscoverBuildArtifacts(context.Background(), &snapshot, []CodeRoot{{root, 10}}, channel.tree)
			if len(next) != 2 || next[0].Rule.snapshot == frozen || next[0].Rule.Observation().Sequence != snapshot.Bundle.Sequence {
				t.Fatal("a new scan reused the previous frozen rules")
			}
		})
	}
}
