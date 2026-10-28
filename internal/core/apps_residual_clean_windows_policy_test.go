package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"
	"unicode/utf16"
)

func windowsCleanFixtureNative(t *testing.T, trash string) windowsResidualCleaner {
	t.Helper()
	return windowsResidualCleaner{
		pending:     func() []uint16 { return nil },
		terminate:   func([]string, func(string) bool) {},
		stopService: func(string) {},
		absent:      func(ResidualKind) (bool, bool) { return false, false },
		remove:      func(ResidualKind) error { t.Fatal("unexpected native mutation"); return nil },
		wait:        func(context.Context, time.Duration) {},
		dispose: func(path string, d Disposal, p *CleanProgress) CleanResult {
			if d != RecycleBin {
				t.Fatal("permanent deletion requested")
			}
			return recyclePathWith(path, p, func(path string) error { return os.Rename(path, filepath.Join(trash, filepath.Base(path))) })
		},
	}
}
func windowsCleanFixtureFile(t *testing.T, path, text string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}
func windowsCleanFixturePending(entries ...string) []uint16 {
	var out []uint16
	for _, entry := range entries {
		out = append(out, utf16.Encode([]rune(entry))...)
		out = append(out, 0)
	}
	return append(out, 0)
}
func TestWindowsResidualCleanPendingRenamePairsAndPrefixes(t *testing.T) {
	words := windowsCleanFixturePending(`\??\C:\first`, "", `*1\??\C:\second`, "", `*2\??\C:\third`, `C:\destination`)
	paths := ParseWindowsPendingRenamePaths(words)
	for _, path := range []string{`c:\first`, `c:\second`, `c:\third`, `c:\destination`} {
		if !paths[path] {
			t.Errorf("missing %q", path)
		}
	}
	if len(paths) != 4 {
		t.Fatalf("unexpected pending paths %v", paths)
	}
	path := filepath.Join(t.TempDir(), "locked")
	windowsCleanFixtureFile(t, path, "pending")
	item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, ResidualOther)
	native := windowsCleanFixtureNative(t, t.TempDir())
	reads := 0
	native.pending = func() []uint16 { reads++; return windowsCleanFixturePending(path) }
	native.terminate = func([]string, func(string) bool) { t.Fatal("pending paths must not trigger process termination") }
	native.dispose = func(string, Disposal, *CleanProgress) CleanResult {
		t.Fatal("pending path recycled")
		return CleanFailed
	}
	report := cleanWindowsResidualsWith(context.Background(), []ResidualItem{item}, NewCleanProgress(1, 0), native)
	if reads != 1 || report.OK != 0 || len(report.Failed) != 0 || len(report.Manual) != 1 {
		t.Fatalf("pending handling: %+v reads=%d", report, reads)
	}
}

func TestWindowsResidualCleanFilesystemSafetyIdentityAndRecycle(t *testing.T) {
	for _, mode := range []string{"selected", "directory", "missing-identity", "replaced", "after-process-shutdown", "after-service-shutdown", "trash-error", "false-trash-success", "cancelled", "protected"} {
		t.Run(mode, func(t *testing.T) {
			root, trash := t.TempDir(), t.TempDir()
			path := filepath.Join(root, "selected")
			windowsCleanFixtureFile(t, path, "old payload")
			kind := ResidualFile
			if mode == "directory" {
				path = root
				kind = ResidualDirectory
			}
			item := CertainResidual(ResidualKind{Type: kind, Path: path}, ResidualOther)
			if item.Identity == nil {
				t.Fatal("identity unavailable")
			}
			native := windowsCleanFixtureNative(t, trash)
			replace := func() {
				if err := os.Rename(path, path+".old"); err != nil {
					t.Fatal(err)
				}
				windowsCleanFixtureFile(t, path, "new payload")
			}
			items := []ResidualItem{item}
			progress := NewCleanProgress(1, 0)
			switch mode {
			case "missing-identity":
				items[0].Identity = nil
			case "replaced":
				replace()
			case "after-process-shutdown":
				native.terminate = func(_ []string, allowed func(string) bool) {
					if !allowed(path) {
						t.Fatal("valid path not authorized")
					}
					replace()
					if allowed(path) {
						t.Fatal("replacement authorized process termination")
					}
				}
			case "after-service-shutdown":
				items = append(items, CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: AppHKLM, Path: `SYSTEM\CurrentControlSet\Services\fixture`}, ResidualService))
				native.stopService = func(string) { replace() }
				native.absent = func(ResidualKind) (bool, bool) { return true, true }
			case "trash-error":
				native.dispose = func(path string, _ Disposal, p *CleanProgress) CleanResult {
					return recyclePathWith(path, p, func(string) error { return errors.New("fixture trash unavailable") })
				}
			case "false-trash-success":
				native.dispose = func(path string, _ Disposal, p *CleanProgress) CleanResult {
					return recyclePathWith(path, p, func(string) error { return nil })
				}
			case "cancelled":
				progress.RequestCancel()
			case "protected":
				items[0].Kind.Path = string(filepath.Separator)
			}
			report := cleanWindowsResidualsWith(context.Background(), items, progress, native)
			if mode == "selected" || mode == "directory" {
				if report.OK != 1 || len(report.Failed) != 0 || progress.Bytes.Load() != 0 || progress.Files.Load() != 1 {
					t.Fatalf("recycle outcome: %+v %+v", report, progress.Snapshot())
				}
				if _, err := os.Lstat(path); !os.IsNotExist(err) {
					t.Fatalf("target remained: %v", err)
				}
				if _, err := os.Lstat(filepath.Join(trash, filepath.Base(path))); err != nil {
					t.Fatal("not restorable in fixture trash", err)
				}
				if ExecutionResult(report) != nil {
					t.Fatal("missing successful capability execution")
				}
			} else {
				if mode != "protected" {
					if _, err := os.Lstat(path); err != nil {
						t.Fatal("unsafe target disappeared", err)
					}
				}
				if mode != "cancelled" && len(report.Failed) == 0 {
					t.Fatalf("unsafe target not reported: %+v", report)
				}
				if mode != "after-service-shutdown" && report.OK != 0 {
					t.Fatalf("false success: %+v", report)
				}
			}
		})
	}
}

func TestWindowsResidualCleanNativeActualPostconditionsAndFailures(t *testing.T) {
	for _, kind := range []ResidualKind{{Type: ResidualRegistryKey, Root: AppHKLM32, Path: `SOFTWARE\fixture`}, {Type: ResidualRegistryValue, Root: AppHKCU, Path: "Environment", Name: "Fixture_HOME"}, {Type: ResidualScheduledTask, Path: `\Fixture\Task`}} {
		for _, mode := range []string{"removed", "already-absent", "unknown-before", "survived-success", "unknown-after", "removed-despite-error", "reappeared-at-verify", "cancel-during-delete"} {
			t.Run(kind.DisplayLabel()+"/"+mode, func(t *testing.T) {
				native := windowsCleanFixtureNative(t, t.TempDir())
				checks, deletes := 0, 0
				gone := mode == "already-absent"
				ctx, cancel := context.WithCancel(context.Background())
				defer cancel()
				native.absent = func(got ResidualKind) (bool, bool) {
					if got != kind {
						t.Fatal("native target kind/view/value changed")
					}
					checks++
					if mode == "unknown-before" || mode == "unknown-after" && deletes > 0 {
						return false, false
					}
					if mode == "reappeared-at-verify" && checks >= 3 {
						return false, true
					}
					return gone, true
				}
				native.remove = func(got ResidualKind) error {
					if got != kind {
						t.Fatal("native target widened")
					}
					deletes++
					gone = mode != "survived-success"
					if mode == "cancel-during-delete" {
						cancel()
					}
					if mode == "removed-despite-error" {
						return errors.New("raced delete")
					}
					return nil
				}
				report := cleanWindowsResidualsWith(ctx, []ResidualItem{CertainResidual(kind, ResidualOther)}, NewCleanProgress(0, 0), native)
				success := slices.Contains([]string{"removed", "already-absent", "removed-despite-error"}, mode)
				if success {
					if report.OK != 1 || len(report.Failed) != 0 || ExecutionResult(report) != nil {
						t.Fatalf("verified absence failed: %+v", report)
					}
				} else {
					if report.OK != 0 || len(report.Failed)+len(report.SkippedItems) == 0 {
						t.Fatalf("unverified success: %+v", report)
					}
					for _, failure := range report.Failed {
						if failure.ID != kind.DisplayLabel() || failure.Path != "" {
							t.Fatalf("native failure cannot be matched by follow-up: %+v", failure)
						}
					}
				}
				if (mode == "already-absent" || mode == "unknown-before") && deletes != 0 {
					t.Fatal("unnecessary/unauthorized mutation")
				}
				if len(report.PlanExecutions) != 1 || len(report.PlanExecutions[0].Steps) != 3 {
					t.Fatal("native execution lacks typed steps")
				}
			})
		}
	}
}

func TestWindowsResidualCleanRuleGenerationAndShutdownOrdering(t *testing.T) {
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	other := clonePlanValue(*snapshot)
	a, b := NewRuleRef(snapshot, "residual-windows", ""), NewRuleRef(&other, "residual-windows", "")
	path := filepath.Join(t.TempDir(), "file")
	windowsCleanFixtureFile(t, path, "keep")
	file := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, ResidualOther)
	file.Rule = &a
	service := CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: AppHKLM, Path: `SYSTEM\CurrentControlSet\Services\FixtureSvc`}, ResidualService)
	service.Rule = &b
	native := windowsCleanFixtureNative(t, t.TempDir())
	native.pending = func() []uint16 { t.Fatal("mixed generation performed native read"); return nil }
	report := cleanWindowsResidualsWith(context.Background(), []ResidualItem{file, service}, NewCleanProgress(1, 0), native)
	if len(report.Failed) != 2 || report.OK != 0 {
		t.Fatalf("mixed generation allowed: %+v", report)
	}
	service.Rule = &a
	native = windowsCleanFixtureNative(t, t.TempDir())
	var events []string
	native.terminate = func(_ []string, allowed func(string) bool) {
		if !allowed(path) || allowed(filepath.Dir(path)+"-sibling"+string(filepath.Separator)+"file") {
			t.Fatal("process authority")
		}
		events = append(events, "process")
	}
	native.stopService = func(name string) {
		if name != "FixtureSvc" {
			t.Fatal("wrong service")
		}
		events = append(events, "service")
	}
	native.absent = func(ResidualKind) (bool, bool) { return true, true }
	dispose := native.dispose
	native.dispose = func(path string, d Disposal, p *CleanProgress) CleanResult {
		events = append(events, "dispose")
		return dispose(path, d, p)
	}
	report = cleanWindowsResidualsWith(context.Background(), []ResidualItem{file, service}, NewCleanProgress(1, 0), native)
	if report.OK != 2 || !slices.Equal(events, []string{"process", "service", "dispose"}) {
		t.Fatalf("shutdown order or reports: %v %+v", events, report)
	}
}
