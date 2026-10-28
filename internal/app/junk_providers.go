package app

import (
	"context"
	"sync"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type junkProviderResult struct {
	targets    []core.ScanTarget
	panicValue any
}

// The join runs on the product worker, preserving its panic recovery boundary.
func startJunkProviders(ctx context.Context, providers ...func() []core.ScanTarget) func() []core.ScanTarget {
	results := make([]junkProviderResult, len(providers))
	var workers sync.WaitGroup
	for i, provider := range providers {
		workers.Go(func() {
			defer func() { results[i].panicValue = recover() }()
			if ctx.Err() == nil {
				results[i].targets = provider()
			}
		})
	}
	return func() []core.ScanTarget {
		workers.Wait()
		var targets []core.ScanTarget
		for _, result := range results {
			if result.panicValue != nil {
				panic(result.panicValue)
			}
			if ctx.Err() == nil {
				targets = append(targets, result.targets...)
			}
		}
		return targets
	}
}
