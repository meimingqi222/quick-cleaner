package core

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func ScanOrphanResiduals(ctx context.Context) (*ResidualScanResult, bool) {
	home, ok := platform.UserHome()
	if !ok {
		return nil, false
	}
	installed, known := platform.InstalledBundleIDs(ctx)
	if !known {
		return nil, false
	}
	installed = lowerBundleIDs(installed)
	snapshot, err := CurrentRules()
	if err != nil {
		return nil, false
	}
	result, err := scanOrphanResidualsIn(ctx, home, installed, snapshot)
	if err != nil {
		return nil, false
	}
	owners := residualOwners(result.Items)
	removed, known := platform.SupplementBundleOwners(ctx, owners, installed)
	if !known {
		return nil, false
	}
	for id := range platform.TrashedBundleIDs(ctx, home) {
		removed[id] = struct{}{}
	}
	if ctx.Err() != nil {
		return nil, false
	}
	classifyOrphanEvidence(&result, installed, removed)
	return &result, true
}

func residualOwners(items []ResidualItem) []string {
	var owners []string
	for _, item := range items {
		if id, ok := item.Owner(); ok {
			owners = append(owners, id)
		}
	}
	return owners
}

func OrphanOwnersAllGone(ctx context.Context, items []ResidualItem) bool {
	if len(items) == 0 {
		return false
	}
	installed, known := platform.InstalledBundleIDs(ctx)
	if !known {
		return false
	}
	installed = lowerBundleIDs(installed)
	if _, known = platform.SupplementBundleOwners(ctx, residualOwners(items), installed); !known {
		return false
	}
	return ctx.Err() == nil && orphanOwnersGone(items, installed)
}
