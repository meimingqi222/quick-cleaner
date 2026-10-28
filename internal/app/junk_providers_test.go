package app

import (
	"context"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func TestJunkProvidersRunTogetherAndKeepOrder(t *testing.T) {
	started := make(chan int, 3)
	release := make(chan struct{})
	released := false
	defer func() {
		if !released {
			close(release)
		}
	}()
	providers := make([]func() []core.ScanTarget, 3)
	for i := range providers {
		providers[i] = func() []core.ScanTarget {
			started <- i
			<-release
			return []core.ScanTarget{{Path: string(rune('a' + i))}}
		}
	}
	finish := startJunkProviders(context.Background(), providers...)
	for range providers {
		select {
		case <-started:
		case <-time.After(3 * time.Second):
			t.Fatal("providers did not start concurrently")
		}
	}
	close(release)
	released = true
	targets := finish()
	if len(targets) != 3 || targets[0].Path != "a" || targets[1].Path != "b" || targets[2].Path != "c" {
		t.Fatalf("provider order changed: %+v", targets)
	}
}

func TestJunkProviderPanicReachesWorker(t *testing.T) {
	finish := startJunkProviders(context.Background(), func() []core.ScanTarget { panic("provider failed") })
	defer func() {
		if recover() != "provider failed" {
			t.Fatal("provider panic lost")
		}
	}()
	_ = finish()
}
