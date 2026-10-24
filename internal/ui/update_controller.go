package ui

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type UpdatePreferences struct {
	AutoCheck      bool
	SkippedVersion *string
	LastCheckAt    *int64
}

type UpdateSnapshot struct {
	Status                            core.UpdateStatus
	Preferences                       UpdatePreferences
	Checking, ShowDialog, ReadyToQuit bool
	ManualCheckError                  string
}

type updateHost interface {
	Packaged() bool
	Cleanup()
	Cache() (string, bool)
	Extract(string, string) (string, error)
	Install(string) error
}

type nativeUpdateHost struct{}

func (nativeUpdateHost) Packaged() bool        { return platform.IsPackagedInstall() }
func (nativeUpdateHost) Cleanup()              { platform.CleanupPreviousUpdateLeftovers() }
func (nativeUpdateHost) Cache() (string, bool) { return platform.UpdateCacheDir() }
func (nativeUpdateHost) Extract(zip, cache string) (string, error) {
	return core.ExtractUpdateZip(zip, cache)
}
func (nativeUpdateHost) Install(payload string) error { return platform.ApplyUpdateAndRestart(payload) }

// UpdateController owns update work, not windows. The product observes Changes,
// reads Snapshot on its UI thread, and quits only after ReadyToQuit. Discovery
// downloads in the background but never installs without an explicit action.
type UpdateController struct {
	mu              sync.Mutex
	state           UpdateSnapshot
	client          *core.UpdateHTTPClient
	host            updateHost
	persist         func(UpdatePreferences)
	ctx             context.Context
	cancel          context.CancelFunc
	changes         chan struct{}
	schedulerCancel context.CancelFunc
}

// persist runs serialized with state changes and must not call back into the
// controller. It updates only update preferences, not unrelated product settings.
func NewUpdateController(version string, prefs UpdatePreferences, persist func(UpdatePreferences)) *UpdateController {
	return newUpdateController(version, prefs, persist, core.NewUpdateHTTPClient(version), nativeUpdateHost{})
}

func newUpdateController(version string, prefs UpdatePreferences, persist func(UpdatePreferences), client *core.UpdateHTTPClient, host updateHost) *UpdateController {
	ctx, cancel := context.WithCancel(context.Background())
	return &UpdateController{
		state:  UpdateSnapshot{Status: core.UpdateStatus{Kind: core.UpdateIdle, CurrentVersion: version}, Preferences: cloneUpdatePreferences(prefs)},
		client: client, host: host, persist: persist, ctx: ctx, cancel: cancel, changes: make(chan struct{}, 1),
	}
}

func cloneUpdatePreferences(p UpdatePreferences) UpdatePreferences {
	if p.SkippedVersion != nil {
		v := *p.SkippedVersion
		p.SkippedVersion = &v
	}
	if p.LastCheckAt != nil {
		v := *p.LastCheckAt
		p.LastCheckAt = &v
	}
	return p
}

func (c *UpdateController) Snapshot() UpdateSnapshot {
	c.mu.Lock()
	defer c.mu.Unlock()
	s := c.state
	s.Preferences = cloneUpdatePreferences(s.Preferences)
	if s.Status.Progress.Total != nil {
		v := *s.Status.Progress.Total
		s.Status.Progress.Total = &v
	}
	return s
}

func (c *UpdateController) Changes() <-chan struct{} { return c.changes }
func (c *UpdateController) notifyLocked() {
	select {
	case c.changes <- struct{}{}:
	default:
	}
}
func (c *UpdateController) saveLocked() {
	if c.persist != nil {
		c.persist(cloneUpdatePreferences(c.state.Preferences))
	}
}

func (c *UpdateController) Close() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.cancel()
	if c.schedulerCancel != nil {
		c.schedulerCancel()
		c.schedulerCancel = nil
	}
}

func (c *UpdateController) SetAutoCheck(enabled bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.state.Preferences.AutoCheck = enabled
	c.saveLocked()
	if !enabled && c.schedulerCancel != nil {
		c.schedulerCancel()
		c.schedulerCancel = nil
	}
	c.notifyLocked()
}

func (c *UpdateController) StartScheduler() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.host.Packaged() || c.ctx.Err() != nil {
		return
	}
	// Backup cleanup still runs when automatic checks are disabled.
	c.host.Cleanup()
	if c.schedulerCancel != nil || !c.state.Preferences.AutoCheck {
		return
	}
	ctx, cancel := context.WithCancel(c.ctx)
	c.schedulerCancel = cancel
	go func() {
		timer := time.NewTimer(time.Duration(core.FirstCheckDelayMS) * time.Millisecond)
		defer timer.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-timer.C:
				c.Check(false)
				timer.Reset(time.Duration(core.CheckIntervalMS) * time.Millisecond)
			}
		}
	}()
}

func (c *UpdateController) Check(manual bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ctx.Err() != nil || !c.host.Packaged() || (!manual && !c.state.Preferences.AutoCheck) || c.state.Checking {
		return
	}
	switch c.state.Status.Kind {
	case core.UpdateDownloading, core.UpdateVerifying, core.UpdateDownloaded, core.UpdateInstalling:
		return
	}
	current := c.state.Status.CurrentVersion
	skipped := cloneUpdatePreferences(c.state.Preferences).SkippedVersion
	c.state.Checking = true
	c.state.Status = core.UpdateStatus{Kind: core.UpdateChecking, CurrentVersion: current}
	c.notifyLocked()
	go func() {
		release, err := c.client.FetchLatestRelease(c.ctx, 15*time.Second)
		var next core.UpdateStatus
		if err == nil {
			next, err = core.EvaluateRelease(current, &release, core.CandidateAssetNames(core.CurrentTarget()))
		}
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.ctx.Err() != nil {
			return
		}
		c.state.Checking = false
		now := time.Now().Unix()
		c.state.Preferences.LastCheckAt = &now
		c.saveLocked()
		if err != nil {
			if manual {
				c.state.ManualCheckError = err.Error()
			}
			c.failLocked(core.UpdateCheck, err)
			return
		}
		if next.Kind == core.UpdateAvailable && core.IsSkipped(next.LatestVersion, skipped) {
			next = core.UpdateStatus{Kind: core.UpdateNotAvailable, CurrentVersion: current}
		}
		c.state.Status = next
		if next.Kind == core.UpdateAvailable {
			c.startDownloadLocked()
		}
		c.notifyLocked()
	}()
}

func (c *UpdateController) failLocked(op core.UpdateOperation, err error) {
	c.state.Status = core.UpdateStatus{Kind: core.UpdateError, CurrentVersion: c.state.Status.CurrentVersion, Operation: op, Message: err.Error()}
	c.notifyLocked()
}

func (c *UpdateController) OpenDialog() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.state.Status.WantsAttention() {
		c.state.ShowDialog = true
		c.notifyLocked()
	}
}

func (c *UpdateController) DismissDialog() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.state.ShowDialog = false
	c.notifyLocked()
}

func (c *UpdateController) SkipVersion() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if v := c.state.Status.LatestVersionValue(); v != nil {
		c.state.Preferences.SkippedVersion = v
		c.saveLocked()
	}
	c.state.ShowDialog = false
	if c.state.Status.Kind == core.UpdateAvailable {
		c.state.Status = core.UpdateStatus{Kind: core.UpdateNotAvailable, CurrentVersion: c.state.Status.CurrentVersion}
	}
	c.notifyLocked()
}

func (c *UpdateController) StartDownload() {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ctx.Err() == nil {
		c.startDownloadLocked()
	}
}

func (c *UpdateController) startDownloadLocked() {
	if c.state.Status.Kind != core.UpdateAvailable {
		return
	}
	cache, ok := c.host.Cache()
	if !ok {
		c.failLocked(core.UpdateDownload, fmt.Errorf("update cache dir unavailable"))
		return
	}
	s := c.state.Status
	s.Kind = core.UpdateDownloading
	c.state.Status = s
	c.notifyLocked()
	go func() {
		zip := filepath.Join(cache, s.AssetName)
		err := c.client.DownloadToFile(c.ctx, s.AssetURL, zip, func(transferred uint64, total *uint64) {
			c.mu.Lock()
			defer c.mu.Unlock()
			if c.ctx.Err() != nil {
				return
			}
			p := core.DownloadProgress{Transferred: transferred, Total: total}
			if total != nil && *total > 0 {
				p.Percent = min(float32(100), float32(transferred)/float32(*total)*100)
			}
			c.state.Status.Progress = p
			c.notifyLocked()
		})
		var payload string
		if err == nil {
			var sidecar string
			sidecar, err = c.client.FetchText(c.ctx, s.ChecksumURL, 20*time.Second)
			if err == nil {
				err = os.WriteFile(filepath.Join(cache, s.AssetName+".sha256"), []byte(sidecar), 0600)
			}
			if err == nil {
				err = core.VerifyChecksum(zip, sidecar)
			}
			if err == nil {
				err = c.ctx.Err()
			}
			if err == nil {
				payload, err = c.host.Extract(zip, cache)
			}
		}
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.ctx.Err() != nil {
			return
		}
		if err != nil {
			c.failLocked(core.UpdateDownload, err)
			return
		}
		c.state.Status = core.UpdateStatus{Kind: core.UpdateDownloaded, CurrentVersion: s.CurrentVersion, LatestVersion: s.LatestVersion, Payload: payload}
		c.notifyLocked()
	}()
}

// Install returns true when the existing busy-work confirmation must be shown.
// Confirmation is per click, not retained after a failed install or a retry.
func (c *UpdateController) Install(busy, confirmed bool) (needsConfirmation bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.ctx.Err() != nil || c.state.Status.Kind != core.UpdateDownloaded {
		return false
	}
	if busy && !confirmed {
		return true
	}
	s := c.state.Status
	c.state.Status = core.UpdateStatus{Kind: core.UpdateInstalling, CurrentVersion: s.CurrentVersion, LatestVersion: s.LatestVersion}
	c.notifyLocked()
	go func() {
		err := c.host.Install(s.Payload)
		c.mu.Lock()
		defer c.mu.Unlock()
		if err != nil {
			c.failLocked(core.UpdateInstall, err)
			return
		}
		// Once the detached helper exists, cancellation must not lose the quit
		// handoff: it is waiting for this process before replacing any files.
		c.state.ReadyToQuit = true
		c.notifyLocked()
	}()
	return false
}

func (c *UpdateController) Retry() {
	c.mu.Lock()
	if c.state.Status.Kind != core.UpdateError {
		c.mu.Unlock()
		return
	}
	c.state.Status = core.UpdateStatus{Kind: core.UpdateIdle, CurrentVersion: c.state.Status.CurrentVersion}
	c.mu.Unlock()
	c.Check(true)
}
