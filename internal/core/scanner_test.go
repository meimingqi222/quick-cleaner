package core

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func TestFixedScanFreezesAuthorityAndPreservesDisplayPolicy(t *testing.T) {
	root := t.TempDir()
	cache, keep := filepath.Join(root, "cache"), filepath.Join(root, "keep")
	for _, path := range []string{cache, keep} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(cache, "data"), []byte("cache bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	prior := CurrentWhitelist()
	defer ReloadWhitelist(prior)
	ReloadWhitelist([]string{keep})
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	target := ProviderTarget(snapshot, "engine", "cache_candidate", cache, NewText("缓存", "Cache"), UserCache, Operation{Kind: OpTree}, true)
	protected := target
	protected.Path = keep
	hint := uint64(12345)
	virtual := ProviderTarget(snapshot, "engine", "docker_image", "docker://image/sha256:fixture", NewText("镜像", "Image"), DockerImages, Operation{Kind: OpDocker, Reference: "sha256:fixture"}, true)
	virtual.SizeHint = &hint
	cats := ScanFixed(context.Background(), []ScanTarget{target, protected, virtual}, nil)
	if len(cats) != 17 {
		t.Fatalf("categories = %d", len(cats))
	}
	var real, native *ScanItem
	for i := range cats {
		for j := range cats[i].Items {
			item := &cats[i].Items[j]
			switch item.Path {
			case cache:
				real = item
			case keep:
				t.Fatal("whitelist target visible")
			default:
				native = item
			}
		}
	}
	if real == nil || real.FileCount != 1 || real.Size == 0 || real.Identity == nil || len(real.Plans) != 1 || !real.Recommended {
		t.Fatalf("real = %+v", real)
	}
	if real.Label.Get(English) != "Cache" || real.Label.Get(Chinese) != "缓存" {
		t.Fatal("bilingual label lost")
	}
	if native == nil || native.Size != hint || native.Identity != nil || native.FileCount != 0 || native.Recommended {
		t.Fatalf("native = %+v", native)
	}
	// The plan must retain the scan-time object, not capture it at clean click.
	if err := os.Rename(cache, cache+"-old"); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(cache, 0700); err != nil {
		t.Fatal(err)
	}
	if real.Plans[0].Validate() == nil {
		t.Fatal("replacement gained scan authority")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for _, cat := range ScanFixed(ctx, []ScanTarget{target}, nil) {
		if len(cat.Items) != 0 {
			t.Fatal("cancelled scan returned new work")
		}
	}
}

func TestNormalizeScanTargetsRetainsCoveredPreservesAndPolicyConflicts(t *testing.T) {
	root := t.TempDir()
	for _, path := range []string{"sessions", "cache", "level1/level2", "other"} {
		if err := os.MkdirAll(filepath.Join(root, path), 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, "config.yaml"), []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	parent := ProviderTarget(snapshot, "engine", "cache_candidate", root, SameText("fixture"), UserCache, Operation{Kind: OpContents}, true)
	child := parent
	child.Path = filepath.Join(root, "sessions")
	child.Rule = NewRuleRef(snapshot, "hermes", root)
	for _, input := range [][]ScanTarget{{parent, child}, {child, parent}} {
		targets := NormalizeScanTargets(input)
		var paths []string
		for _, item := range targets {
			paths = append(paths, filepath.Base(item.Path))
			if item.Operation.Kind != OpTree {
				t.Fatalf("child operation = %v", item.Operation)
			}
			if len(item.Rule.Preserved()) == 0 {
				t.Fatal("covered preserve constraints lost")
			}
		}
		if !reflect.DeepEqual(paths, []string{"cache", "level1", "other"}) {
			t.Fatalf("split paths = %v", paths)
		}
	}
	conflict := parent
	conflict.Disposal = RecycleBin
	for _, input := range [][]ScanTarget{{parent, conflict}, {conflict, parent}} {
		targets := NormalizeScanTargets(input)
		if len(targets) != 1 || targets[0].Recommended {
			t.Fatalf("conflict targets = %+v", targets)
		}
		if err := NewCleanupPlan(targets[0].Rule, nil).Validate(); err == nil {
			t.Fatal("conflict authorized")
		}
	}
}

func TestMergeDiscoveredAndApplyCleanResult(t *testing.T) {
	root := t.TempDir()
	cache, nested, independent := filepath.Join(root, "cache"), filepath.Join(root, "cache", "build"), filepath.Join(root, "independent")
	for _, path := range []string{nested, independent} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	cats := AggregateScanItems([]ScanItem{{Path: cache, Category: UserCache, Size: 30}})
	MergeDiscovered(cats, []ScanItem{{Path: nested, Category: DevBuild, Size: 20}, {Path: independent, Category: DevBuild, Size: 70}}, true)
	for _, cat := range cats {
		if cat.Category == DevBuild && (!cat.Partial || cat.TotalSize != 70 || len(cat.Items) != 1) {
			t.Fatalf("discovery = %+v", cat)
		}
		if cat.Category != DevBuild && cat.Partial {
			t.Fatal("fixed category marked partial")
		}
	}
	cleared := ApplyCleanResult(cats, []string{cache, independent}, []string{independent})
	if !reflect.DeepEqual(cleared, []string{cache}) {
		t.Fatalf("cleared = %v", cleared)
	}
	for _, cat := range cats {
		if cat.Category == DevBuild && cat.TotalSize != 70 {
			t.Fatal("failed target disappeared")
		}
	}
	// Even an empty discovery category must disclose the exhausted budget.
	empty := AggregateScanItems(nil)
	MergeDiscovered(empty, nil, true)
	for _, cat := range empty {
		if cat.Category == DevBuild && !cat.Partial {
			t.Fatal("empty partial category concealed")
		}
	}
}

func TestRulePathTargetsTrustedRootsAndMinimumAge(t *testing.T) {
	root := t.TempDir()
	old, young := filepath.Join(root, "old"), filepath.Join(root, "young")
	for _, path := range []string{old, young} {
		if err := os.Mkdir(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	then := time.Now().Add(-2 * time.Hour)
	if err := os.Chtimes(old, then, then); err != nil {
		t.Fatal(err)
	}
	snapshot := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{{ID: "fixture", Version: 1, Platform: "all", Entries: []PathRule{
		{Root: "home", Path: "old", Zh: "旧", En: "Old", Category: "UserCache", Operation: "contents", Disposal: Permanent, Recommended: true, MinimumAgeSeconds: 3600},
		{Root: "home", Path: "young", Zh: "新", En: "Young", Category: "UserCache", Operation: "contents", Disposal: Permanent, Recommended: true, MinimumAgeSeconds: 3600},
		{Root: "local", Path: "absent", Operation: "contents", Disposal: Permanent},
	}}}}}
	targets := RulePathTargets(snapshot, map[string]string{"home": root}, "macos")
	if len(targets) != 2 || !targets[0].Recommended || targets[1].Recommended {
		t.Fatalf("age/root targets = %+v", targets)
	}
	if targets := RulePathTargets(snapshot, nil, "macos"); len(targets) != 0 {
		t.Fatalf("unknown home expanded: %+v", targets)
	}
	snapshot.Bundle.Rules[0].Platform = "windows"
	if targets := RulePathTargets(snapshot, map[string]string{"home": root}, "macos"); len(targets) != 0 {
		t.Fatal("wrong-platform rule expanded")
	}
}
