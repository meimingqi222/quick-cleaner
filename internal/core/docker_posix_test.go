//go:build linux || darwin

package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestDockerSelectedTargetRequiresVerifiedAbsenceAndRealLayerDeletion(t *testing.T) {
	for _, mode := range []string{"deleted", "untagged", "still-present", "bad-inventory", "bad-utf8", "probe-failed", "remove-failed", "id-still-present", "cancelled", "invalid", "recycle"} {
		t.Run(mode, func(t *testing.T) {
			root := t.TempDir()
			const script = `#!/bin/sh
if [ "$1" = "container" ]; then exit 0; fi
if [ "$1" = "image" ]; then
  if [ "$#" != 4 ] || [ "$2" != "rm" ] || [ "$3" != "--" ]; then exit 90; fi
  if [ "$4" != "example/app:old" ] && [ "$4" != "aaaaaaaaaaaa" ]; then exit 91; fi
  printf '%s\n' "$4" > "$QC_DOCKER_ROOT/mutated"
  if [ "$QC_DOCKER_MODE" = "remove-failed" ]; then printf 'refused\n' >&2; exit 5; fi
  if [ "$QC_DOCKER_MODE" = "untagged" ]; then printf 'Untagged: example/app:old\n'; else printf 'Deleted: layer\n'; fi
  exit 0
fi
if [ "$1" != "images" ]; then exit 92; fi
if [ -f "$QC_DOCKER_ROOT/mutated" ]; then
  if [ "$2" != "--no-trunc" ]; then exit 93; fi
  case "$QC_DOCKER_MODE" in
    bad-inventory) printf 'unreadable inventory\n'; exit 0 ;;
    bad-utf8) printf '\377'; exit 0 ;;
    probe-failed) exit 9 ;;
  esac
else
  printf 'aaaaaaaaaaaa|example/app|old|<none>|1MB\n'
fi
if [ "$QC_DOCKER_MODE" = "still-present" ] && [ -f "$QC_DOCKER_ROOT/mutated" ]; then
  printf 'aaaaaaaaaaaa|example/app|old|<none>|1MB\n'
fi
printf 'aaaaaaaaaaaa|example/app|keep|<none>|1MB\n'
`
			if err := os.WriteFile(filepath.Join(root, "docker"), []byte(script), 0700); err != nil {
				t.Fatal(err)
			}
			t.Setenv("PATH", root)
			t.Setenv("QC_DOCKER_ROOT", root)
			t.Setenv("QC_DOCKER_MODE", mode)
			s, err := CurrentRules()
			if err != nil {
				t.Fatal(err)
			}
			targets := DockerScanTargets(s)
			if len(targets) != 2 || targets[0].Recommended || targets[0].Category != DockerImages || targets[0].Disposal != Permanent || targets[0].SizeHint == nil || *targets[0].SizeHint != 1000000 {
				t.Fatalf("provider: %+v", targets)
			}
			categories := ScanFixed(context.Background(), targets, nil)
			var selected CleanTarget
			for _, category := range categories {
				for _, item := range category.Items {
					if item.Path == DockerImagePath("example/app:old") {
						selected = item.CleanTarget()
					}
				}
			}
			if len(selected.Plans) != 1 || selected.Identity != nil {
				t.Fatal("native target did not retain its frozen plan")
			}
			if mode == "id-still-present" || mode == "invalid" || mode == "recycle" {
				reference := "aaaaaaaaaaaa"
				if mode == "invalid" {
					reference = "--force"
				}
				item := ScanItem{Path: DockerImagePath(reference), Rule: EngineRuleRef(), Operation: Operation{Kind: OpDocker, Reference: reference}, Disposal: Permanent, Size: 1000000}
				if mode == "recycle" {
					item.Disposal = RecycleBin
				}
				selected = item.FreezePlan().CleanTarget()
			}
			progress := NewCleanProgress(1, 1000000)
			if mode == "cancelled" {
				progress.RequestCancel()
			}
			report := CleanTargets([]CleanTarget{selected}, progress)
			success := mode == "deleted" || mode == "untagged"
			if (report.OK == 1 && len(report.Failed) == 0 && ExecutionResult(report) == nil) != success {
				t.Fatalf("mode %s report %+v", mode, report)
			}
			wantBytes, wantFiles := uint64(0), uint64(0)
			if mode == "deleted" {
				wantBytes = 1000000
			}
			if success {
				wantFiles = 1
			}
			if progress.Snapshot().Bytes != wantBytes || progress.Snapshot().Files != wantFiles {
				t.Fatalf("unverified or untag-only removal credited space: %+v", progress.Snapshot())
			}
			if mode == "cancelled" || mode == "invalid" || mode == "recycle" {
				if _, err := os.Stat(filepath.Join(root, "mutated")); !os.IsNotExist(err) {
					t.Fatal("unauthorized mutation attempted")
				}
			}
		})
	}
}
