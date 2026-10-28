package core

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestFrozenFilesystemPlanExecutesAndRejectsChangedSelection(t *testing.T) {
	path := filepath.Join(t.TempDir(), "target")
	identityWrite(t, path, "selected")
	id := identityCapture(t, path)
	target := PlannedTarget{Path: path, Operation: Operation{Kind: OpFile}, Identity: &id, Disposal: Permanent}
	plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{target})
	if err := plan.Validate(); err != nil {
		t.Fatal(err)
	}
	changed := target.Operation
	changed.Kind = OpTree
	if err := plan.ValidateBinding(path, changed, Permanent, &id); err == nil {
		t.Fatal("changed operation accepted")
	}
	if err := plan.ValidateBinding(path, target.Operation, RecycleBin, &id); err == nil {
		t.Fatal("changed disposal accepted")
	}
	// Returned diagnostic values must not mutate execution authority.
	copy := plan.Targets()
	copy[0].Path = filepath.Join(filepath.Dir(path), "not-selected")
	*copy[0].Identity = TargetIdentity{}
	r := ExecuteFilesystemPlan(plan, 0, &CleanProgress{}, SpotClear)
	if err := ExecutionResult(r); err != nil {
		t.Fatalf("execution: %v; %+v", err, r)
	}
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("selected file remains: %v", err)
	}
}

func TestPlanRejectsBusyUnknownMissingIdentityAndReplacement(t *testing.T) {
	for _, state := range []SpotCheck{SpotBusy, SpotUnknown} {
		path := filepath.Join(t.TempDir(), "keep")
		identityWrite(t, path, "keep")
		id := identityCapture(t, path)
		plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{{Path: path, Operation: Operation{Kind: OpFile}, Identity: &id, Disposal: Permanent}})
		r := ExecuteFilesystemPlan(plan, 0, &CleanProgress{}, state)
		if ExecutionResult(r) == nil || r.OK != 0 || len(r.Failed) != 1 {
			t.Fatalf("occupancy=%v report=%+v", state, r)
		}
		if _, err := os.Stat(path); err != nil {
			t.Fatal(err)
		}
	}
	path := filepath.Join(t.TempDir(), "keep")
	identityWrite(t, path, "old")
	target := PlannedTarget{Path: path, Operation: Operation{Kind: OpFile}, Disposal: Permanent}
	if err := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{target}).Validate(); err == nil {
		t.Fatal("missing identity accepted")
	}
	id := identityCapture(t, path)
	target.Identity = &id
	plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{target})
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, path, "replacement")
	if err := plan.Validate(); err == nil {
		t.Fatal("replacement accepted")
	}
}

func TestPlanPreserveContributorsAndConflictsInBothOrders(t *testing.T) {
	root := t.TempDir()
	keep := filepath.Join(root, "keep")
	if err := os.Mkdir(keep, 0700); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, filepath.Join(keep, "sentinel"), "preserved")
	snapshot := &RuleSnapshot{Bundle: RuleBundle{Schema: RuleSchema, Sequence: 1, Rules: []RuleDefinition{{ID: "one", Version: 1}, {ID: "two", Version: 1, Preserve: []string{"keep"}}}}}
	one, two := NewRuleRef(snapshot, "one", root), NewRuleRef(snapshot, "two", root)
	for _, rule := range []RuleRef{one.Merge(two), two.Merge(one)} {
		for _, path := range []string{root, filepath.Join(keep, "sentinel")} {
			id := identityCapture(t, path)
			plan := NewCleanupPlan(rule, []PlannedTarget{{Path: path, Operation: Operation{Kind: OpTree}, Identity: &id, Disposal: Permanent}})
			if err := plan.Validate(); err == nil || !strings.Contains(err.Error(), "preserved path") {
				t.Fatalf("preserve not enforced for %s: %v", path, err)
			}
		}
	}
	id := identityCapture(t, root)
	a := PlannedTarget{Path: root, Operation: Operation{Kind: OpTree}, Identity: &id, Disposal: Permanent}
	b := a
	b.Operation.Kind = OpContents
	for _, targets := range [][]PlannedTarget{{a, b}, {b, a}} {
		if err := NewCleanupPlan(one, targets).Validate(); err == nil || !strings.Contains(err.Error(), "Conflicting") {
			t.Fatalf("conflict accepted: %v", err)
		}
	}
}

func TestFilesystemCompletionDoesNotConfuseMissingWithEmpty(t *testing.T) {
	root := t.TempDir()
	missing := filepath.Join(root, "missing")
	for _, test := range []struct {
		condition CompletionCondition
		want      Evidence
	}{
		{CompletionCondition{Kind: "path_absent", Path: missing}, Confirmed},
		{CompletionCondition{Kind: "contents_empty", Path: missing}, Unknown},
		{CompletionCondition{Kind: "contents_empty", Path: root}, Confirmed},
		{CompletionCondition{Kind: "docker_reference_absent", Reference: "example:1"}, Unknown},
	} {
		if got := FilesystemCompletion(test.condition); got != test.want {
			t.Fatalf("%+v got %s want %s", test.condition, got, test.want)
		}
	}
	identityWrite(t, filepath.Join(root, "present"), "present")
	if got := FilesystemCompletion(CompletionCondition{Kind: "contents_empty", Path: root}); got != Absent {
		t.Fatalf("nonempty reported %s", got)
	}
}

func TestInstallationAndOfficialOperationFreezeEvidence(t *testing.T) {
	root := t.TempDir()
	cwd := filepath.Join(root, "cwd")
	if err := os.Mkdir(cwd, 0700); err != nil {
		t.Fatal(err)
	}
	exe := filepath.Join(root, "runtime")
	identityWrite(t, exe, "executable evidence")
	command := OfficialUninstaller{Executable: exe, WorkingDirectory: cwd, Arguments: []string{"--preserve"}}
	op, err := CaptureOfficialOperation(command, nil)
	if err != nil {
		t.Fatal(err)
	}
	command.Arguments[0] = "--changed"
	frozen, err := op.Command()
	if err != nil || frozen.Arguments[0] != "--preserve" {
		t.Fatalf("command not frozen: %+v %v", frozen, err)
	}
	frozen.Arguments[0] = "--mutated-copy"
	frozen, err = op.Command()
	if err != nil || frozen.Arguments[0] != "--preserve" {
		t.Fatalf("returned command aliases authority: %+v %v", frozen, err)
	}
	if err := os.Rename(cwd, cwd+".old"); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(cwd, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := op.Command(); err == nil {
		t.Fatal("replaced working directory accepted")
	}
	absent := filepath.Join(root, "absent-at-scan")
	instance, err := CaptureInstallation(root, []string{exe, absent})
	if err != nil {
		t.Fatal(err)
	}
	if err := instance.ValidateLivePaths(root, []string{filepath.Join(root, "new-claim")}); err == nil {
		t.Fatal("new installation claim accepted")
	}
	identityWrite(t, absent, "appeared later")
	if err := instance.Revalidate(); err == nil {
		t.Fatal("new artifact accepted")
	}
	if err := os.Remove(absent); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(exe); err != nil {
		t.Fatal(err)
	}
	if err := instance.Revalidate(); err != nil {
		t.Fatalf("confirmed disappearance prevents recovery: %v", err)
	}
}

func TestTypedOperationExtentAndNativeParameters(t *testing.T) {
	for _, path := range []string{"/fixture/go/pkg/mod", "/fixture/pnpm/store", "/fixture/.pnpm-store"} {
		if (Operation{Kind: OpTree}).ValidateTarget(path, true) || (Operation{Kind: OpContents}).ValidateTarget(path, false) {
			t.Fatalf("owner path accepted as raw deletion: %s", path)
		}
	}
	if !(Operation{Kind: OpGo}).ValidateTarget("/fixture/go/pkg/mod", false) || (Operation{Kind: OpGo}).ValidateTarget("/fixture/go/pkg/mod-other", false) {
		t.Fatal("Go owner boundary differs")
	}
	if !(Operation{Kind: OpPnpm}).ValidateTarget("/fixture/pnpm/store", false) || !(Operation{Kind: OpPnpm}).ValidateTarget("/fixture/.pnpm-store", false) {
		t.Fatal("pnpm owner layout missing")
	}
	for _, bad := range []string{"-all", "image;touch", "image$(command)", "repo/image tag", "中文", strings.Repeat("a", 513)} {
		if ValidDockerReference(bad) {
			t.Fatalf("invalid Docker argument accepted: %q", bad)
		}
	}
	for _, valid := range []string{"repo/image:2", "sha256:0123456789", "registry:5000/repo/image@sha256:1234"} {
		if !ValidDockerReference(valid) {
			t.Fatalf("valid Docker reference refused: %s", valid)
		}
	}
	for _, invalid := range []string{"--delete-all", "2026-02-30-120000", "2026-10-04-256000", "prefix.2026-10-04-120000.local"} {
		if _, ok := SnapshotDate(invalid); ok {
			t.Fatalf("invalid snapshot accepted: %s", invalid)
		}
	}
	name := "com.apple.TimeMachine.2026-10-04-120000.local"
	if date, ok := SnapshotDate(name); !ok || date != "2026-10-04-120000" {
		t.Fatalf("snapshot date: %s %v", date, ok)
	}
	a := Operation{Kind: OpDocker, Reference: "repo/image:2"}
	if a.TargetKey("docker://image/display-one") != a.TargetKey("docker://image/display-two") {
		t.Fatal("display URI became execution identity")
	}
	if a.ValidateTarget(filepath.Join(t.TempDir(), "real-selection"), false) {
		t.Fatal("real selection converted to native operation")
	}
}
