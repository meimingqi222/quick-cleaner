package app

import (
	"context"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func TestStatusSamplingOnlyOnTheVisibleStatusPage(t *testing.T) {
	r, _ := testRoot(t)
	r.statusWake = make(chan struct{}, 1)
	r.syncStatusSampling()
	if r.statusWanted.Load() {
		t.Fatal("the overview must not sample processes")
	}
	r.active = view.Status
	r.syncStatusSampling()
	select {
	case <-r.statusWake:
	default:
		t.Fatal("entering the Status page must sample at once")
	}
	for name, hide := range map[string]func(){
		"hidden window": func() { r.windowHidden = true },
		"paused":        func() { r.dashboard.MonitoringPaused = true },
		"other page":    func() { r.active = view.Junk },
	} {
		r.active, r.windowHidden, r.dashboard.MonitoringPaused = view.Status, false, false
		r.syncStatusSampling()
		hide()
		r.syncStatusSampling()
		if r.statusWanted.Load() {
			t.Fatalf("%s still samples every two seconds", name)
		}
	}
}

func TestStatusLoopSamplesOnlyWhenWantedOrAsked(t *testing.T) {
	r, _ := testRoot(t)
	tick := make(chan time.Time)
	refresh, wake := make(chan struct{}), make(chan struct{})
	sampled := make(chan struct{}, 8)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go r.runStatusLoop(ctx, tick, refresh, wake, func(context.Context) (core.StatusSnapshot, error) {
		sampled <- struct{}{}
		return core.StatusSnapshot{}, nil
	})
	expect := func(want bool, what string) {
		t.Helper()
		select {
		case <-sampled:
			if !want {
				t.Fatalf("%s sampled", what)
			}
		case <-time.After(100 * time.Millisecond):
			if want {
				t.Fatalf("%s did not sample", what)
			}
		}
	}
	tick <- time.Now()
	expect(false, "a tick off the Status page")
	refresh <- struct{}{}
	expect(true, "a manual refresh")
	r.statusWanted.Store(true)
	wake <- struct{}{}
	expect(true, "becoming wanted")
	tick <- time.Now()
	expect(true, "a tick on the Status page")
}
