package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func leafFixtureTarget(t *testing.T, dir string, op Operation) CleanTarget {
	t.Helper()
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	target := ProviderTarget(rules, "engine", "cache_candidate", dir, SameText("cache"), UserCache, op, true)
	cats := ScanFixed(context.Background(), []ScanTarget{target}, nil)
	for _, cat := range cats {
		for _, item := range cat.Items {
			return item.CleanTarget()
		}
	}
	t.Fatalf("fixture scan found nothing: %+v", cats)
	return CleanTarget{}
}

func writeLeaf(t *testing.T, path string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("cache bytes"), 0600); err != nil {
		t.Fatal(err)
	}
}

// One locked file used to forfeit the whole cache directory. Contents cleanup
// must delete everything else and report only the held leaf as a skip.
func TestContentsCleanupSkipsOnlyLockedLeaves(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "cache")
	free := []string{filepath.Join(dir, "a.bin"), filepath.Join(dir, "b.bin"), filepath.Join(dir, "sub", "d.bin")}
	held := filepath.Join(dir, "sub", "c.bin")
	for _, path := range append(free, held) {
		writeLeaf(t, path)
	}
	target := leafFixtureTarget(t, dir, Operation{Kind: OpContents})
	f, err := os.Open(held)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()

	p := NewCleanProgress(4, 0)
	report := CleanTargets([]CleanTarget{target}, p)

	for _, path := range free {
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			t.Errorf("unlocked leaf kept: %s (%v)", path, err)
		}
	}
	if _, err := os.Lstat(held); err != nil {
		t.Fatalf("held leaf was deleted: %v", err)
	}
	if len(report.FailInfo) != 1 {
		t.Fatalf("fail info = %+v", report.FailInfo)
	}
	info := report.FailInfo[0]
	if info.Reason != FailInUse || info.FailedLeaves == 0 || info.SkippedLeaves != info.FailedLeaves {
		t.Fatalf("locked leaf not reported as a pure skip: %+v", info)
	}
	if failed, _ := p.SplitFailures(report.Failed); len(failed) != 0 {
		t.Fatalf("locked leaf surfaced as a real failure: %+v", failed)
	}
	snap := p.Snapshot()
	if snap.Files != 3 || snap.Ignored != 1 || snap.Failed != 1 {
		t.Fatalf("progress files=%d ignored=%d failed=%d; want 3/1/1 (ancestors must not recount)", snap.Files, snap.Ignored, snap.Failed)
	}
}

// A real error next to a locked file must still surface as a failure.
func TestFailParentPrefersRealErrorsOverSkips(t *testing.T) {
	p := NewCleanProgress(0, 0)
	p.fail("/d/locked", FailInUse)
	p.fail("/d/denied", FailAccessDenied)
	p.failParent("/d", []string{"/d/locked", "/d/denied"})
	p.failParent("/e", []string{"/d/locked"})
	failed, skipped := p.SplitFailures([]CleanFailure{{Path: "/d"}, {Path: "/e"}})
	if len(failed) != 1 || failed[0].Path != "/d" || len(skipped) != 1 || skipped[0].Path != "/e" {
		t.Fatalf("failed=%v skipped=%v", failed, skipped)
	}
}

// Whole trees (node_modules, target/) stay all-or-nothing: a partly deleted
// dependency tree under a running project is worse than an untouched one.
func TestTreeTargetsKeepWholeTargetOccupancyGate(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "tree")
	free := filepath.Join(dir, "a.bin")
	held := filepath.Join(dir, "held.bin")
	writeLeaf(t, free)
	writeLeaf(t, held)
	target := leafFixtureTarget(t, dir, Operation{Kind: OpTree})
	if leafOccupancyGate(target) {
		t.Fatal("tree target routed to per-leaf cleanup")
	}
	f, err := os.Open(held)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	CleanTargets([]CleanTarget{target}, NewCleanProgress(2, 0))
	if _, err := os.Lstat(free); err != nil {
		t.Fatalf("tree partially deleted around a lock: %v", err)
	}
}
