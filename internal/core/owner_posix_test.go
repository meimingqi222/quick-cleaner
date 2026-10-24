//go:build linux || darwin

package core

import (
	"os"
	"path/filepath"
	"testing"
)

func TestOwnerPlanVerifiesCompletionAndLimitsFallbackScope(t *testing.T) {
	for _, mode := range []string{"success", "false-success", "failed", "scope-mismatch"} {
		t.Run(mode, func(t *testing.T) {
			root := t.TempDir()
			target := filepath.Join(root, "go", "pkg", "mod")
			if err := os.MkdirAll(target, 0700); err != nil {
				t.Fatal(err)
			}
			identityWrite(t, filepath.Join(target, "module"), "fixture")
			other := filepath.Join(root, "other-store")
			if err := os.Mkdir(other, 0700); err != nil {
				t.Fatal(err)
			}
			identityWrite(t, filepath.Join(other, "keep"), "not selected")
			tools := filepath.Join(root, "tools")
			if err := os.Mkdir(tools, 0700); err != nil {
				t.Fatal(err)
			}
			const script = `#!/bin/sh
if [ "$1" = "env" ]; then
  if [ "$QC_OWNER_MODE" = "scope-mismatch" ]; then
    printf '%s\n' "$QC_OWNER_OTHER"
  else
    printf '%s\n' "$QC_OWNER_TARGET"
  fi
  exit 0
fi
if [ "$1" != "clean" ] || [ "$2" != "-modcache" ]; then exit 91; fi
if [ "$QC_OWNER_MODE" = "success" ]; then
  /bin/mv "$QC_OWNER_TARGET" "$QC_OWNER_TARGET.moved"
elif [ "$QC_OWNER_MODE" = "failed" ]; then
  printf '%s\n' 'owner refused' >&2
  exit 7
elif [ "$QC_OWNER_MODE" = "scope-mismatch" ]; then
  exit 92
fi
exit 0
`
			if err := os.WriteFile(filepath.Join(tools, "go"), []byte(script), 0700); err != nil {
				t.Fatal(err)
			}
			t.Setenv("PATH", tools)
			t.Setenv("QC_OWNER_MODE", mode)
			t.Setenv("QC_OWNER_TARGET", target)
			t.Setenv("QC_OWNER_OTHER", other)
			id := identityCapture(t, target)
			plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{{Path: target, Operation: Operation{Kind: OpGo}, Identity: &id, Disposal: Permanent}})
			p := &CleanProgress{}
			report := ExecuteOwnerPlan(plan, 0, p, SpotClear, 37)
			succeeded := mode == "success" || mode == "scope-mismatch"
			if (ExecutionResult(report) == nil) != succeeded {
				t.Fatalf("mode %s report=%+v", mode, report)
			}
			if _, err := os.Stat(filepath.Join(other, "keep")); err != nil {
				t.Fatalf("unselected owner scope changed: %v", err)
			}
			switch mode {
			case "success":
				if p.Snapshot().Bytes != 37 {
					t.Fatalf("owner completion not accounted: %+v", p.Snapshot())
				}
			case "failed", "false-success":
				if p.Snapshot().Bytes != 0 {
					t.Fatal("unverified owner operation credited bytes")
				}
				if _, err := os.Stat(filepath.Join(target, "module")); err != nil {
					t.Fatalf("attempted owner failure fell back to raw delete: %v", err)
				}
			case "scope-mismatch":
				entries, err := os.ReadDir(target)
				if err != nil || len(entries) != 0 {
					t.Fatalf("authorized contents fallback failed: %v %v", entries, err)
				}
			}
		})
	}
}
