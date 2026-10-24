package ui

import (
	"context"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

type trayOverviewFixture struct {
	samples, purges, volumes atomic.Int32
	mu                       sync.Mutex
	thermal                  []bool
	accountRelease           chan struct{}
	purgeRelease             chan struct{}
	purgeSuccess             atomic.Bool
}

func (f *trayOverviewFixture) NewSampler(context.Context) (trayOverviewSampler, error) { return f, nil }
func (f *trayOverviewFixture) Sample(ctx context.Context, slow bool) (core.TraySnapshot, error) {
	if err := ctx.Err(); err != nil {
		return core.TraySnapshot{}, err
	}
	n := f.samples.Add(1)
	f.mu.Lock()
	f.thermal = append(f.thermal, slow)
	f.mu.Unlock()
	return core.TraySnapshot{CPUPct: uint32(n), MemoryUsed: uint64(n), MemoryTotal: 100}, nil
}
func (f *trayOverviewFixture) Accounts() []core.LocalAccount {
	if f.accountRelease != nil {
		<-f.accountRelease
	}
	return []core.LocalAccount{{Name: "current", Path: "/fixture", IsCurrent: true}}
}
func (f *trayOverviewFixture) Volumes() []TrayVolumeInfo {
	f.volumes.Add(1)
	return []TrayVolumeInfo{{Label: "fixture", TotalBytes: 123, FreeBytes: 45}}
}
func (f *trayOverviewFixture) ReleaseMemory() bool {
	f.purges.Add(1)
	if f.purgeRelease != nil {
		<-f.purgeRelease
	}
	return f.purgeSuccess.Load()
}

func awaitTrayOverview(t *testing.T, c *TrayOverviewController, match func(TrayOverviewSnapshot) bool) TrayOverviewSnapshot {
	t.Helper()
	timer := time.NewTimer(3 * time.Second)
	defer timer.Stop()
	for {
		if state := c.Snapshot(); match(state) {
			return state
		}
		select {
		case <-c.Changes():
		case <-timer.C:
			t.Fatal("tray state timeout")
		}
	}
}

func TestTrayOverviewRefreshDoesNotWaitForAccountsAndStopsOnClose(t *testing.T) {
	f := &trayOverviewFixture{accountRelease: make(chan struct{})}
	c := newTrayOverviewController(f)
	c.warmup, c.period = time.Millisecond, 5*time.Millisecond
	t.Cleanup(c.Close)
	c.Start()
	c.Start()
	first := awaitTrayOverview(t, c, func(s TrayOverviewSnapshot) bool { return s.MemoryUsed >= 2 })
	if first.CPUPct < 2 || len(first.Accounts) != 0 || len(first.Volumes) != 1 {
		t.Fatalf("first snapshot: %+v", first)
	}
	close(f.accountRelease)
	awaitTrayOverview(t, c, func(s TrayOverviewSnapshot) bool { return s.MemoryUsed >= 7 && len(s.Accounts) == 1 })
	f.mu.Lock()
	flags := append([]bool(nil), f.thermal...)
	f.mu.Unlock()
	if flags[0] || !flags[1] || flags[2] || flags[3] || flags[4] || flags[5] || !flags[6] {
		t.Fatalf("thermal cadence: %v", flags)
	}
	c.Close()
	closed := c.Snapshot()
	time.Sleep(30 * time.Millisecond)
	if after := c.Snapshot(); after.MemoryUsed != closed.MemoryUsed {
		t.Fatal("closed model refreshed")
	}
	stopped := f.samples.Load()
	time.Sleep(30 * time.Millisecond)
	if f.samples.Load() != stopped {
		t.Fatal("closed window still samples")
	}
	// Snapshots are detached; consumers must not mutate controller data.
	closed.Accounts[0].Name = "changed"
	closed.Volumes[0].Label = "changed"
	if got := c.Snapshot(); got.Accounts[0].Name != "current" || got.Volumes[0].Label != "fixture" {
		t.Fatal("snapshot aliases model")
	}
}

func TestTrayOverviewReleaseMemoryStateAndActionRouting(t *testing.T) {
	f := &trayOverviewFixture{purgeRelease: make(chan struct{})}
	f.purgeSuccess.Store(true)
	c := newTrayOverviewController(f)
	c.feedback = 20 * time.Millisecond
	t.Cleanup(c.Close)
	c.Start()
	var dispatched []OverviewAction
	dispatch := func(a OverviewAction) { dispatched = append(dispatched, a) }
	for _, kind := range []OverviewActionKind{OverviewOpenMain, OverviewScanJunk, OverviewOpenView, OverviewAnalyzeDisk, OverviewQuit} {
		a := OverviewAction{Kind: kind, View: Disk, Path: "/fixture"}
		if !a.DismissesPopover() {
			t.Fatal("navigation must dismiss")
		}
		c.Handle(a, dispatch)
	}
	if len(dispatched) != 5 || dispatched[3].Path != "/fixture" {
		t.Fatal(dispatched)
	}
	release := OverviewAction{Kind: OverviewReleaseMemory}
	if release.DismissesPopover() {
		t.Fatal("memory release must keep popover open")
	}
	c.Handle(release, dispatch)
	c.Handle(release, dispatch)
	if state := c.Snapshot(); !state.Purging || state.Purged || state.PurgeFailed {
		t.Fatal(state)
	}
	close(f.purgeRelease)
	awaitTrayOverview(t, c, func(s TrayOverviewSnapshot) bool { return s.Purged })
	c.Handle(release, dispatch)
	if f.purges.Load() != 1 || len(dispatched) != 5 {
		t.Fatal("duplicate purge or action dispatch")
	}
	awaitTrayOverview(t, c, func(s TrayOverviewSnapshot) bool { return !s.Purged })
	f.purgeSuccess.Store(false)
	c.Handle(release, dispatch)
	awaitTrayOverview(t, c, func(s TrayOverviewSnapshot) bool { return s.PurgeFailed && !s.Purging })
	if f.purges.Load() != 2 {
		t.Fatal("failure not observed")
	}
}
