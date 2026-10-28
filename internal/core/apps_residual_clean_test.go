package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func macResidualFixtureCleaner(trash string) macResidualCleaner {
	return macResidualCleaner{
		needsPrivilege: func(string) bool { return false },
		bootout:        func(context.Context, string) {},
		deactivate:     func(context.Context, ResidualKind) CleanResult { return CleanManualAction },
		elevated: func(_ context.Context, paths []string, allowed func(string) bool) map[string]bool {
			removed := map[string]bool{}
			for _, path := range paths {
				if allowed(path) && os.Remove(path) == nil {
					removed[path] = true
				}
			}
			return removed
		},
		dispose: func(path string, disposal Disposal, progress *CleanProgress) CleanResult {
			if disposal != RecycleBin {
				return Dispose(path, disposal, progress)
			}
			return recyclePathWith(path, progress, func(path string) error { return os.Rename(path, filepath.Join(trash, filepath.Base(path))) })
		},
	}
}

func TestMacResidualCleanupRequiresCurrentIdentityAndKnownVacancy(t *testing.T) {
	for _, mode := range []string{"selected", "missing-identity", "replaced", "busy", "unknown", "no-probe", "canceled"} {
		t.Run(mode, func(t *testing.T) {
			root := t.TempDir()
			trash := t.TempDir()
			path := filepath.Join(root, "selected-state")
			sourceFixtureFile(t, path, "selected data")
			item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, ResidualAppSupportDir)
			if item.Identity == nil {
				t.Fatal("fixture has no identity")
			}
			spot := map[string]SpotCheck{path: SpotClear}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			switch mode {
			case "missing-identity":
				item.Identity = nil
			case "replaced":
				if err := os.Rename(path, path+".old"); err != nil {
					t.Fatal(err)
				}
				sourceFixtureFile(t, path, "replacement")
			case "busy":
				spot[path] = SpotBusy
			case "unknown":
				spot[path] = SpotUnknown
			case "no-probe":
				delete(spot, path)
			case "canceled":
				cancel()
			}
			progress := NewCleanProgress(1, 0)
			report := cleanMacResidualsWith(ctx, []ResidualItem{item}, progress, spot, macResidualFixtureCleaner(trash))
			if mode == "selected" {
				data, err := os.ReadFile(filepath.Join(trash, filepath.Base(path)))
				if report.OK != 1 || len(report.Failed) != 0 || err != nil || string(data) != "selected data" {
					t.Fatalf("selected file must be restorable in trash: %+v data=%q err=%v", report, data, err)
				}
				if _, err := os.Lstat(path); !os.IsNotExist(err) || progress.Files.Load() != 1 || progress.Bytes.Load() != 0 {
					t.Fatalf("recycling must verify disappearance without claiming released bytes: %+v %v", progress.Snapshot(), err)
				}
			} else {
				if _, err := os.Lstat(path); err != nil || report.OK != 0 || mode != "canceled" && len(report.Failed) != 1 {
					t.Fatalf("unsafe/canceled target must survive: %+v %v", report, err)
				}
			}
		})
	}
}

func TestMacResidualCleanupRechecksAfterLaunchAgentShutdown(t *testing.T) {
	path := filepath.Join(t.TempDir(), "com.example.agent.plist")
	sourceFixtureFile(t, path, "registered agent")
	item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, ResidualLaunchAgent)
	native := macResidualFixtureCleaner(t.TempDir())
	native.bootout = func(context.Context, string) {
		if err := os.Rename(path, path+".old"); err != nil {
			t.Fatal(err)
		}
		sourceFixtureFile(t, path, "new agent registered by shutdown")
	}
	report := cleanMacResidualsWith(context.Background(), []ResidualItem{item}, NewCleanProgress(1, 0), map[string]SpotCheck{path: SpotClear}, native)
	data, err := os.ReadFile(path)
	if report.OK != 0 || len(report.Failed) != 1 || err != nil || string(data) != "new agent registered by shutdown" {
		t.Fatalf("shutdown replacement must not inherit deletion authority: %+v %q %v", report, data, err)
	}
}

func TestMacResidualCleanupRejectsMixedSnapshotsAndNonwhitelistedElevation(t *testing.T) {
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	other := clonePlanValue(*snapshot)
	a, b := NewRuleRef(snapshot, "residual-macos", ""), NewRuleRef(&other, "residual-macos", "")
	path := filepath.Join(t.TempDir(), "owned-data")
	sourceFixtureFile(t, path, "retain")
	item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, ResidualAppSupportDir)
	item.Rule = &a
	extension := CertainResidual(ResidualKind{Type: ResidualSystemExtension, BundleID: "com.example.driver"}, ResidualExtensionSource)
	extension.Rule = &b
	native := macResidualFixtureCleaner(t.TempDir())
	report := cleanMacResidualsWith(context.Background(), []ResidualItem{item, extension}, NewCleanProgress(1, 0), map[string]SpotCheck{path: SpotClear}, native)
	if report.OK != 0 || len(report.Failed) != 2 || len(report.Manual) != 0 {
		t.Fatalf("a mixed generation must reject the whole batch: %+v", report)
	}
	native.needsPrivilege = func(string) bool { return true }
	report = cleanMacResidualsWith(context.Background(), []ResidualItem{item}, NewCleanProgress(1, 0), map[string]SpotCheck{path: SpotClear}, native)
	if _, err := os.Lstat(path); err != nil || report.OK != 0 || len(report.Failed) != 1 {
		t.Fatalf("access failure cannot authorize an arbitrary privileged path: %+v %v", report, err)
	}
	extension.Rule = &a
	report = cleanMacResidualsWith(context.Background(), []ResidualItem{extension}, NewCleanProgress(0, 0), nil, native)
	if report.OK != 0 || len(report.Manual) != 1 || report.Manual[0].ID != extension.Kind.BundleID || report.Manual[0].Path != "" {
		t.Fatalf("manual extension must use an opaque ID, never a filesystem path: %+v", report)
	}
}
