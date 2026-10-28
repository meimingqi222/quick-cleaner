//go:build linux || darwin

package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestSnapshotCleanupVerifiesExactSnapshotAndDoesNotEstimateSpace(t *testing.T) {
	for _, mode := range []string{"removed", "still-present", "unknown", "failed", "invalid"} {
		t.Run(mode, func(t *testing.T) {
			root := t.TempDir()
			const script = `#!/bin/sh
if [ "$1" = "deletelocalsnapshots" ]; then
  if [ "$#" != 2 ] || [ "$2" != "2026-10-04-120000" ]; then exit 90; fi
  printf 'attempted\n' > "$QC_SNAPSHOT_ROOT/mutated"
  if [ "$QC_SNAPSHOT_MODE" = "failed" ]; then exit 3; fi
  exit 0
fi
if [ "$1" != "listlocalsnapshots" ] || [ "$2" != "/" ]; then exit 91; fi
if [ -f "$QC_SNAPSHOT_ROOT/mutated" ]; then
  if [ "$QC_SNAPSHOT_MODE" = "unknown" ]; then printf 'unrecognized inventory\n'; exit 0; fi
  printf 'Snapshots for disk (/):\ncom.apple.TimeMachine.2026-10-05-120000.local\n'
  if [ "$QC_SNAPSHOT_MODE" != "still-present" ]; then exit 0; fi
else
  printf 'Snapshots for disk (/):\n'
fi
printf 'com.apple.TimeMachine.2026-10-04-120000.local\n'
`
			if err := os.WriteFile(filepath.Join(root, "tmutil"), []byte(script), 0700); err != nil {
				t.Fatal(err)
			}
			t.Setenv("PATH", root)
			t.Setenv("QC_SNAPSHOT_ROOT", root)
			t.Setenv("QC_SNAPSHOT_MODE", mode)
			s, err := CurrentRules()
			if err != nil {
				t.Fatal(err)
			}
			targets := MacOSSnapshotTargets(s)
			if len(targets) != 1 || targets[0].Category != LocalSnapshots || targets[0].SizeHint != nil {
				t.Fatalf("snapshot provider: %+v", targets)
			}
			var selected CleanTarget
			for _, cat := range ScanFixed(context.Background(), targets, nil) {
				if cat.Category == LocalSnapshots && len(cat.Items) == 1 {
					selected = cat.Items[0].CleanTarget()
				}
			}
			if mode == "invalid" {
				item := ScanItem{Rule: EngineRuleRef(), Path: SnapshotPath("--all"), Operation: Operation{Kind: OpSnapshot, Name: "--all"}, Disposal: Permanent}
				selected = item.FreezePlan().CleanTarget()
			}
			progress := NewCleanProgress(1, 123)
			report := CleanTargets([]CleanTarget{selected}, progress)
			success := mode == "removed"
			if (ExecutionResult(report) == nil && report.OK == 1 && len(report.Failed) == 0) != success || progress.Bytes.Load() != 0 || (progress.Files.Load() == 1) != success {
				t.Fatalf("snapshot mode %s: %+v %+v", mode, report, progress.Snapshot())
			}
			if mode == "invalid" {
				if _, err := os.Stat(filepath.Join(root, "mutated")); !os.IsNotExist(err) {
					t.Fatal("invalid snapshot reached mutation")
				}
			}
		})
	}
}
