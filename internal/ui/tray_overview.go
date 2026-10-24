package ui

import (
	"context"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type OverviewActionKind uint8

const (
	OverviewOpenMain OverviewActionKind = iota
	OverviewScanJunk
	OverviewOpenView
	OverviewAnalyzeDisk
	OverviewTopMemory
	OverviewQuit
)

type OverviewAction struct {
	Kind OverviewActionKind
	View View
	Path string
}

// Every current action hands off to the main window. An in-place action would
// return false here and be handled by the controller instead of dispatched.
func (a OverviewAction) DismissesPopover() bool { return true }

type TrayVolumeInfo struct {
	Label                 string
	TotalBytes, FreeBytes uint64
}

type TrayOverviewSnapshot struct {
	core.TraySnapshot
	Volumes  []TrayVolumeInfo
	Accounts []core.LocalAccount
}

type trayOverviewSampler interface {
	Sample(context.Context, bool) (core.TraySnapshot, error)
}

type trayOverviewSource interface {
	NewSampler(context.Context) (trayOverviewSampler, error)
	Volumes() []TrayVolumeInfo
	Accounts() []core.LocalAccount
}

type nativeTrayOverviewSource struct{}

func (nativeTrayOverviewSource) NewSampler(ctx context.Context) (trayOverviewSampler, error) {
	return core.NewTraySampler(ctx)
}
func (nativeTrayOverviewSource) Accounts() []core.LocalAccount { return core.ListLocalAccounts() }
func (nativeTrayOverviewSource) Volumes() []TrayVolumeInfo {
	var volumes []TrayVolumeInfo
	for _, volume := range platform.ListVolumes() {
		if total, free, ok := platform.GetVolumeSpace(volume); ok {
			volumes = append(volumes, TrayVolumeInfo{Label: volume.Display(), TotalBytes: total, FreeBytes: free})
		}
	}
	return volumes
}

// One model per popover lifetime. Start and Close must follow actual window
// mount/removal, not rendering or reachability of an old presentation snapshot.
// The host observes Changes and reads Snapshot on its UI thread.
type TrayOverviewController struct {
	mu             sync.Mutex
	state          TrayOverviewSnapshot
	source         trayOverviewSource
	ctx            context.Context
	cancel         context.CancelFunc
	started        bool
	changes        chan struct{}
	warmup, period time.Duration
}

func NewTrayOverviewController() *TrayOverviewController {
	return newTrayOverviewController(nativeTrayOverviewSource{})
}

func newTrayOverviewController(source trayOverviewSource) *TrayOverviewController {
	ctx, cancel := context.WithCancel(context.Background())
	return &TrayOverviewController{source: source, ctx: ctx, cancel: cancel, changes: make(chan struct{}, 1), warmup: 200 * time.Millisecond, period: 2 * time.Second}
}

func (c *TrayOverviewController) Changes() <-chan struct{} { return c.changes }

func (c *TrayOverviewController) notifyLocked() {
	select {
	case c.changes <- struct{}{}:
	default:
	}
}

func (c *TrayOverviewController) Snapshot() TrayOverviewSnapshot {
	c.mu.Lock()
	defer c.mu.Unlock()
	out := c.state
	out.Volumes = append([]TrayVolumeInfo(nil), out.Volumes...)
	out.Accounts = append([]core.LocalAccount(nil), out.Accounts...)
	for i := range out.Accounts {
		if p := out.Accounts[i].SizeBytes; p != nil {
			n := *p
			out.Accounts[i].SizeBytes = &n
		}
	}
	if out.TempC != nil {
		temp := *out.TempC
		out.TempC = &temp
	}
	return out
}

func (c *TrayOverviewController) Start() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.started || c.ctx.Err() != nil {
		return
	}
	c.started = true
	go func() {
		if c.ctx.Err() != nil {
			return
		}
		accounts := c.source.Accounts()
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.ctx.Err() != nil {
			return
		}
		c.state.Accounts = accounts
		c.notifyLocked()
	}()
	go c.refresh()
}

func (c *TrayOverviewController) refresh() {
	if c.ctx.Err() != nil {
		return
	}
	sampler, err := c.source.NewSampler(c.ctx)
	if err != nil {
		return
	}
	// Prime without publishing: first displayed sample needs a real CPU delta.
	if _, err := sampler.Sample(c.ctx, false); err != nil {
		return
	}
	timer := time.NewTimer(c.warmup)
	defer timer.Stop()
	for tick := uint64(0); ; tick++ {
		select {
		case <-c.ctx.Done():
			return
		case <-timer.C:
		}
		if c.ctx.Err() != nil {
			return
		}
		slow := tick%5 == 0
		snapshot, err := sampler.Sample(c.ctx, slow)
		if c.ctx.Err() != nil {
			return
		}
		var volumes []TrayVolumeInfo
		if slow {
			volumes = c.source.Volumes()
		}
		c.mu.Lock()
		if c.ctx.Err() != nil {
			c.mu.Unlock()
			return
		}
		if err == nil {
			c.state.TraySnapshot = snapshot
		}
		if slow {
			c.state.Volumes = volumes
		}
		c.notifyLocked()
		c.mu.Unlock()
		timer.Reset(c.period)
	}
}

func (c *TrayOverviewController) Close() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.cancel()
}

// The tray host handles dismissal from the action contract; content never
// directly closes windows, starts workers, or mutates this model.
func (c *TrayOverviewController) Handle(action OverviewAction, dispatch func(OverviewAction)) {
	if dispatch != nil {
		dispatch(action)
	}
}
