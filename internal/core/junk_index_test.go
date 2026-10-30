package core

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"
)

func TestJunkIndexedDiscoveryUsesExistingVolumeAndSkipsMFTWhenUnelevated(t *testing.T) {
	home := buildProjectFixture(t)
	volumeRoot := buildProjectFixture(t)
	modules := filepath.Join(volumeRoot, "code", "proj", "node_modules")
	payload := filepath.Join(modules, "pkg.dat")
	if err := os.MkdirAll(modules, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(volumeRoot, "code", "proj", "package.json"), []byte(`{}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(payload, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	if AtOrUnder(Norm(modules), Norm(home)) {
		t.Fatal("fixture volume is inside the user home")
	}
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	homeOnly := DiscoverBuildArtifacts(context.Background(), rules, CodeRoots(home, nil), nil)
	for _, item := range homeOnly {
		if Norm(item.Path) == Norm(modules) {
			t.Fatal("home-only discovery saw the other volume")
		}
	}
	const indexedSize = 424242
	volume := VolumeFromMountPoint(volumeRoot)
	tree := SizeTreeFromSnapshot(volume, []TreeSnapshotEntry{
		{Path: filepath.Join(volumeRoot, "code"), IsDir: true},
		{Path: filepath.Join(volumeRoot, "code", "proj"), IsDir: true},
		{Path: filepath.Join(volumeRoot, "code", "proj", "package.json"), Size: 2},
		{Path: modules, IsDir: true},
		{Path: payload, Size: indexedSize},
	})
	target := ScanTarget{Path: modules, Category: DevBuild, Operation: Operation{Kind: OpTree}}
	opened := map[string]int{}
	acquire := func(_ context.Context, v VolumeID) (*SizeTree, error) {
		opened[normMount(v.MountPoint())]++
		return nil, nil
	}

	elevated := BeginJunkIndex(context.Background(), true, home, []VolumeID{volume}, []*SizeTree{tree}, []ScanTarget{target}, acquire)
	if len(opened) != 0 {
		t.Fatalf("elevated indexed scan opened a volume: %v", opened)
	}
	cats := ScanFixed(context.Background(), []ScanTarget{target}, elevated.Query)
	measured, ok := junkItemSize(cats, modules)
	if !ok || measured != indexedSize {
		t.Fatalf("phase-1 size = %d ok=%v; want indexed %d", measured, ok, indexedSize)
	}
	found := discoverPath(t, elevated.Discover(context.Background(), rules), modules)
	if found.Size != indexedSize {
		t.Fatalf("indexed discovery size = %d; want %d", found.Size, indexedSize)
	}

	opened = map[string]int{}
	unelevated := BeginJunkIndex(context.Background(), false, home, []VolumeID{volume}, nil, []ScanTarget{target}, acquire)
	items := unelevated.Discover(context.Background(), rules)
	if len(opened) != 0 {
		t.Fatalf("unelevated discovery opened a volume: %v", opened)
	}
	if _, letter := volume.DriveLetter(); letter {
		live := discoverPath(t, items, modules)
		if live.Size == indexedSize {
			t.Fatal("unelevated walk reported the index size")
		}
	}

	opened = map[string]int{}
	missing := BeginJunkIndex(context.Background(), true, home, []VolumeID{volume}, nil, []ScanTarget{target}, acquire)
	_ = missing.Discover(context.Background(), rules)
	if opened[normMount(volume.MountPoint())] == 0 {
		t.Fatalf("elevated scan without an index did not acquire the volume: %v", opened)
	}
}

func junkItemSize(cats []CategorySummary, path string) (uint64, bool) {
	for _, cat := range cats {
		for _, item := range cat.Items {
			if Norm(item.Path) == Norm(path) {
				return item.Size, true
			}
		}
	}
	return 0, false
}

func discoverPath(t *testing.T, items []ScanItem, path string) ScanItem {
	t.Helper()
	for _, item := range items {
		if Norm(item.Path) == Norm(path) {
			return item
		}
	}
	t.Fatalf("discovery missed %s (%d items)", path, len(items))
	return ScanItem{}
}

func TestDiscoverWithinMarksOnlyBudgetExpiryPartial(t *testing.T) {
	home := buildProjectFixture(t)
	project := filepath.Join(home, "code", "app")
	if err := os.MkdirAll(filepath.Join(project, "node_modules", "dep"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(project, "package.json"), []byte(`{"name":"app"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(project, "node_modules", "dep", "index.js"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	plan := BeginJunkIndex(context.Background(), false, home, nil, nil, nil, nil)
	items, expired := plan.DiscoverWithin(context.Background(), rules, time.Hour)
	if expired || len(items) != 1 {
		t.Fatalf("a finished discovery is complete: expired=%v items=%d", expired, len(items))
	}
	if _, expired := plan.DiscoverWithin(context.Background(), rules, 0); !expired {
		t.Fatal("an exhausted budget must report partial results")
	}
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, expired := plan.DiscoverWithin(cancelled, rules, time.Hour); expired {
		t.Fatal("a user cancel is not a partial result")
	}
}

func TestPartialIndexDiscoveryWalksOnlyUncoveredBranches(t *testing.T) {
	home, err := filepath.EvalSymlinks(buildProjectFixture(t))
	if err != nil {
		t.Fatal(err)
	}
	healthy := filepath.Join(home, "work", "healthy")
	missing := filepath.Join(home, "work", "unknown")
	for _, dir := range []string{healthy, filepath.Join(missing, "node_modules")} {
		if err := os.MkdirAll(dir, 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(missing, "package.json"), []byte(`{}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(missing, "node_modules", "payload"), []byte("data"), 0600); err != nil {
		t.Fatal(err)
	}
	tree := SizeTreeFromSnapshot(VolumeFromMountPoint(home), []TreeSnapshotEntry{
		{Path: filepath.Join(home, "work"), IsDir: true},
		{Path: healthy, IsDir: true},
	})
	tree.SetBlockedSubtrees([]string{missing})
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	previous := ruleLstat
	defer func() { ruleLstat = previous }()
	var probes atomic.Int64
	ruleLstat = func(path string) (os.FileInfo, error) {
		if pathWithin(path, healthy) {
			probes.Add(1)
		}
		return previous(path)
	}
	plan := BeginJunkIndex(context.Background(), false, home, nil, []*SizeTree{tree}, nil, nil)
	items, expired := plan.DiscoverWithin(context.Background(), rules, time.Minute)
	if expired {
		t.Fatal("small discovery exhausted budget")
	}
	discoverPath(t, items, filepath.Join(missing, "node_modules"))
	if n := probes.Load(); n != 0 {
		t.Errorf("walk re-probed fully indexed branch %d times", n)
	}
}

func TestJunkCoveragePreparesGapsOncePerDiscovery(t *testing.T) {
	root := t.TempDir()
	volume := VolumeFromMountPoint(root)
	tree := NewSizeTree(volume, []TreeIndexEntry{{Used: true, IsDir: true, Name: "root"}, {Used: true, IsDir: true, Name: "healthy"}, {Used: true, IsDir: true, Name: "project"}, {Parent: 2, Used: true, IsDir: true, Name: "cloud"}, {Parent: 2, Used: true, IsDir: true, Name: "cloud-x"}})
	blocked := []string{filepath.Join(root, "project", "cloud", "unknown")}
	for i := range 128 {
		blocked = append(blocked, filepath.Join(root, fmt.Sprintf("OTHER-GAP-%d", i), "UNKNOWN"))
	}
	tree.SetBlockedSubtrees(blocked)
	covered := newJunkCoverage([]*SizeTree{tree})
	path := filepath.Join(root, "healthy")
	if allocs := testing.AllocsPerRun(3, func() {
		if !covered(path) {
			panic("healthy branch lost coverage")
		}
	}); allocs > 30 {
		t.Errorf("coverage query repeated per-gap normalization: %.0f allocations", allocs)
	}
	for _, path := range []string{root, filepath.Join(root, "healthy"), filepath.Join(root, "project"), filepath.Join(root, "project", "CLOUD"), filepath.Join(root, "project", "cloud-x"), filepath.Join(root, "project", "cloud", "unknown"), filepath.Join(root, "absent")} {
		if got, want := covered(path), tree.CoversSubtree(path); got != want {
			t.Errorf("coverage(%q)=%v, want %v", path, got, want)
		}
	}
}
