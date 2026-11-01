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

func TestParallelCleanerDeletesHardlinkNamesWithoutFalseOccupancy(t *testing.T) {
	original := filepath.Join(t.TempDir(), "keep")
	if err := os.WriteFile(original, []byte("cache"), 0600); err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(t.TempDir(), "cache")
	for i := 0; i < 128; i++ {
		dir := filepath.Join(root, fmt.Sprint(i/8))
		if err := os.MkdirAll(dir, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.Link(original, filepath.Join(dir, fmt.Sprint(i))); err != nil {
			t.Fatal(err)
		}
	}
	p := NewCleanProgress(128, 0)
	if result := DeleteTree(root, p); result != CleanOK || p.Files.Load() != 128 || p.Failed.Load() != 0 {
		t.Fatalf("hardlink cleanup=%v progress=%+v reasons=%v", result, p.Snapshot(), p.reasons)
	}
	if _, err := os.Lstat(root); !os.IsNotExist(err) {
		t.Fatalf("root remains: %v", err)
	}
	if content, err := os.ReadFile(original); err != nil || string(content) != "cache" {
		t.Fatalf("outside link changed: %q %v", content, err)
	}
}

func TestCleanerParallelBudgetCoversNestedWorkAndDrains(t *testing.T) {
	p := &CleanProgress{}
	var active, peak, finished atomic.Int32
	p.cleanParallel(16, func(int) {
		p.cleanParallel(16, func(int) {
			n := active.Add(1)
			for old := peak.Load(); n > old && !peak.CompareAndSwap(old, n); old = peak.Load() {
			}
			time.Sleep(time.Millisecond)
			active.Add(-1)
			finished.Add(1)
		})
	})
	if peak.Load() < 2 || peak.Load() > 8 || active.Load() != 0 || finished.Load() != 256 {
		t.Fatalf("peak=%d active=%d finished=%d; want 2..8, 0, 256", peak.Load(), active.Load(), finished.Load())
	}
}

func TestCleanTargetsSurvivesDirectoryChurnAndVanishedFiles(t *testing.T) {
	root := t.TempDir()
	cacheDir := filepath.Join(root, "go-build")
	if err := os.MkdirAll(filepath.Join(cacheDir, "00"), 0700); err != nil {
		t.Fatal(err)
	}
	oldStamp := time.Unix(1700000000, 0)
	if err := os.WriteFile(filepath.Join(cacheDir, "00", "stale.o"), []byte("stale"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(cacheDir, oldStamp, oldStamp); err != nil {
		t.Fatal(err)
	}
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	target := ProviderTarget(snapshot, "engine", "cache_candidate", cacheDir, SameText("go-build"), UserCache, Operation{Kind: OpContents}, true)
	cats := ScanFixed(context.Background(), []ScanTarget{target}, nil)
	var cleanTarget CleanTarget
	for _, cat := range cats {
		if len(cat.Items) > 0 {
			cleanTarget = cat.Items[0].CleanTarget()
			break
		}
	}
	if cleanTarget.Path == "" {
		t.Fatalf("unexpected scan result: %+v", cats)
	}

	// Background tool adds a file after scan, advancing directory mtime.
	if err := os.WriteFile(filepath.Join(cacheDir, "new-after-scan.o"), []byte("fresh"), 0600); err != nil {
		t.Fatal(err)
	}
	later := oldStamp.Add(10 * time.Second)
	if err := os.Chtimes(cacheDir, later, later); err != nil {
		t.Fatal(err)
	}

	p := NewCleanProgress(2, 10)
	// Vanished file must count as CleanSkipped, never FailOther.
	if got := cleanFile(filepath.Join(root, "already-vanished.tmp"), 4, p); got != CleanSkipped || p.Failed.Load() != 0 {
		t.Fatalf("vanished file result=%v failed=%d; want CleanSkipped with 0 failures", got, p.Failed.Load())
	}

	report := CleanTargets([]CleanTarget{cleanTarget}, p)
	failed, _ := p.SplitFailures(report.Failed)
	if len(failed) != 0 || report.OK == 0 {
		t.Fatalf("directory churn caused false cleanup failure: report=%+v failed=%v", report, failed)
	}
}
