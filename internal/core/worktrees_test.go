package core

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestWorktreeCleanupKeepsDirtyLockedAndSiblingCheckouts(t *testing.T) {
	root := t.TempDir()
	main := filepath.Join(root, "main repo")
	if err := os.Mkdir(main, 0700); err != nil {
		t.Fatal(err)
	}
	git := func(args ...string) string {
		t.Helper()
		run, ok := RunToolWithTimeout("git", append([]string{"-C", main}, args...), 15*time.Second)
		if !ok || !run.OK {
			t.Fatalf("git %v: %s", args, run.Stderr)
		}
		return strings.TrimSpace(string(run.Stdout))
	}
	git("init")
	if err := os.WriteFile(filepath.Join(main, "tracked.txt"), []byte("committed data"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(main, ".gitignore"), []byte("ignored.bin\n"), 0600); err != nil {
		t.Fatal(err)
	}
	git("add", ".")
	git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "fixture")
	selected, sibling := filepath.Join(root, "trees", "selected checkout"), filepath.Join(root, "trees", "sibling")
	git("worktree", "add", "--detach", selected, "HEAD")
	git("worktree", "add", "--detach", sibling, "HEAD")
	entry, err := InspectWorktree(selected)
	if err != nil {
		t.Fatal(err)
	}
	other, err := InspectWorktree(sibling)
	if err != nil {
		t.Fatal(err)
	}
	if reason := entry.CheckReady(); reason != "" {
		t.Fatalf("clean fixture not ready: %s", reason)
	}
	if paths := DiscoverWorktrees(filepath.Join(root, "trees")); len(paths) != 2 {
		t.Fatalf("discover = %v", paths)
	}
	operation := (Operation{Kind: OpTree}).ForScannedPath(selected)
	if operation.Kind != OpGitWorktree || operation.Registration != entry.Admin {
		t.Fatalf("scan operation = %+v", operation)
	}
	makePlan := func() *CleanupPlan {
		id, ok := CaptureIdentity(selected)
		if !ok {
			t.Fatal("identity unavailable")
		}
		return NewCleanupPlan(EngineRuleRef(), []PlannedTarget{{Path: selected, Operation: operation, Disposal: Permanent, Identity: &id}})
	}
	for _, name := range []string{"untracked.txt", "ignored.bin"} {
		path := filepath.Join(selected, name)
		if err := os.WriteFile(path, []byte("user data"), 0600); err != nil {
			t.Fatal(err)
		}
		if reason := entry.CheckReady(); reason != FailWorktreeDirty {
			t.Fatalf("%s readiness = %s", name, reason)
		}
		report := ExecuteWorktreePlan(makePlan(), 0, &CleanProgress{}, SpotClear)
		if len(report.Failed) != 1 {
			t.Fatalf("dirty cleanup = %+v", report)
		}
		if _, err := os.Stat(path); err != nil {
			t.Fatal("dirty data was touched")
		}
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
	}
	lock := filepath.Join(entry.Admin, "locked")
	if err := os.WriteFile(lock, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	if reason := entry.CheckReady(); reason != FailWorktreeLocked {
		t.Fatalf("lock readiness = %s", reason)
	}
	if report := ExecuteWorktreePlan(makePlan(), 0, &CleanProgress{}, SpotClear); len(report.Failed) != 1 {
		t.Fatalf("locked cleanup = %+v", report)
	}
	if err := os.Remove(lock); err != nil {
		t.Fatal(err)
	}
	plan := makePlan()
	p := &CleanProgress{}
	report := ExecuteWorktreePlan(plan, 0, p, SpotClear)
	if len(report.Failed) != 0 || len(report.SkippedItems) != 0 || p.Files.Load() != 3 {
		t.Fatalf("clean checkout cleanup = %+v, progress = %+v", report, p.Snapshot())
	}
	for _, path := range []string{selected, entry.Admin} {
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			t.Fatalf("selected path still present: %s, %v", path, err)
		}
	}
	for _, path := range []string{sibling, other.Admin, filepath.Join(main, "tracked.txt")} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("unselected data touched: %s: %v", path, err)
		}
	}
	if len(report.PlanExecutions) != 1 || len(report.PlanExecutions[0].Steps) != 5 {
		t.Fatal("worktree lifecycle incomplete")
	}
	for _, step := range report.PlanExecutions[0].Steps {
		if step.Status != StepSucceeded {
			t.Fatalf("incomplete step: %+v", step)
		}
	}
}
