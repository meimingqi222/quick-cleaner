package cli

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestJunkLockAfterScanIsSkippedWithoutFailure(t *testing.T) {
	path := writeFixture(t, t.TempDir(), "cache/data.bin", "keep locked data")
	rules, err := core.CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	target := core.ProviderTarget(rules, "engine", "cache_candidate", filepath.Dir(path), core.SameText("cache"), core.UserCache, core.Operation{Kind: core.OpContents}, true)
	cats := core.ScanFixed(context.Background(), []core.ScanTarget{target}, nil)
	var items []core.ScanItem
	for _, cat := range cats {
		items = append(items, cat.Items...)
	}
	if len(items) != 1 {
		t.Fatalf("fixture scan: %+v", cats)
	}
	f, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	// Inspect reporting without writing the real user's history.
	p := core.NewCleanProgress(1, items[0].Size)
	report := core.CleanTargets([]core.CleanTarget{items[0].CleanTarget()}, p)
	result := cleanupOutcome(report, p)
	if len(result.Failed) != 0 || result.Skipped != 1 || len(result.SkippedItems) != 1 {
		t.Fatalf("late occupancy reported as failure: %+v", result)
	}
	if result.DeletedFiles != 0 {
		t.Fatal("locked data deleted")
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatal("locked data was mutated")
	}
}
