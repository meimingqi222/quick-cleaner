package app

import (
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// identifyCleanupOccupants runs on the cleanup worker, never on the UI thread.
// It only explains FailInUse targets; attribution is display evidence and is
// deliberately kept separate from the cleaner's fail-closed occupancy gate.
func identifyCleanupOccupants(report core.CleanReport) map[string][]platform.Occupant {
	seen := map[string]bool{}
	var paths []string
	for _, info := range report.FailInfo {
		if info.Reason != core.FailInUse || seen[core.Norm(info.Path)] {
			continue
		}
		seen[core.Norm(info.Path)] = true
		paths = append(paths, info.Path)
	}
	if len(paths) == 0 {
		return nil
	}
	deadline := time.Now().Add(4 * time.Second)
	out := map[string][]platform.Occupant{}
	var mu sync.Mutex
	var wg sync.WaitGroup
	slots := make(chan struct{}, 4)
collect:
	for _, path := range paths {
		if !time.Now().Before(deadline) {
			break
		}
		select {
		case slots <- struct{}{}:
		case <-time.After(time.Until(deadline)):
			break collect
		}
		wg.Add(1)
		go func(path string) {
			defer wg.Done()
			defer func() { <-slots }()
			owners, err := platform.OccupancyOwners(path)
			if err != nil || len(owners) == 0 {
				return
			}
			mu.Lock()
			out[core.Norm(path)] = owners
			mu.Unlock()
		}(path)
	}
	wg.Wait()
	if len(out) == 0 {
		return nil
	}
	return out
}
