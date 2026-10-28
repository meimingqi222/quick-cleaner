package app

import (
	"runtime"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/assets"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// Run retains one root for the lifetime of the main window. Closing while a
// mutation is pending requests cancellation and waits for its truthful outcome.
func Run() error {
	mygo.App.SetName("QuickCleaner")
	mygo.App.SetVersion(Version)
	// Windows takes its session-wide lock in main, before elevating. Elsewhere
	// MyGo's per-user lock hands a second launch over to this instance.
	if runtime.GOOS != "windows" && !mygo.App.RequestSingleInstanceLock() {
		return nil
	}
	mygo.App.WhenReady(func() {
		// Rust sets the Dock icon from assets/icon-512.png before the first
		// window. SetIcon needs the running backend, so it goes here.
		if runtime.GOOS == "darwin" {
			_ = mygo.App.Dock.SetIcon(assets.Icon512PNG)
		}
		var window *mygo.Window
		r := NewRoot(core.LoadSettings(), mygo.RunOnMain, func() {
			if window != nil && !window.IsDestroyed() {
				window.Invalidate()
			}
		})
		opts := mygo.WindowOptions{Title: "QuickCleaner", Width: 1280, Height: 880, Content: ui.View(r.Render)}
		prepareStartupWindow(&opts)
		window = mygo.NewWindow(opts)
		r.window = window
		window.OnClose(func(e *mygo.CloseEvent) { r.onMainClose(e, window.Hide) })
		window.OnClosed(func() { r.Close(); mygo.App.Quit() })
		visibility := func(hidden bool) func() {
			return func() { r.windowHidden = hidden; r.syncStatusSampling() }
		}
		window.OnHide(visibility(true))
		window.OnMinimize(visibility(true))
		window.OnShow(visibility(false))
		window.OnRestore(visibility(false))
		mygo.App.OnBeforeQuit(r.onBeforeQuit)
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { r.Close() })
		mygo.App.OnActivate(func(bool) {
			if !r.closed {
				r.showMain()
			}
		})
		wake := func() {
			if !r.closed {
				r.showMain()
			}
		}
		mygo.App.OnSecondInstance(func([]string, string) { wake() })
		platform.OnInstanceWake(func() { mygo.RunOnMain(wake) })
		r.startUpdates()
		r.installMenu()
		if runtime.GOOS == "darwin" {
			// Seed the badge before the first paint. The sampler also reports
			// access, but the onboarding decision cannot wait for that tick.
			if platform.HasFullDiskAccess() {
				r.dashboard.FullDiskAccess = true
			} else if !r.settings.MacOSFDADismissed {
				r.permission.Open = true
				r.checkPermission()
			}
		}
		r.startTray()
		r.startTrashWatch()
		r.startMonitoring()
		r.loadVolumes()
		// After the close handlers exist. Windows stays cloaked until the first frame.
		finishStartupWindow(window)
	})
	return mygo.App.Run()
}

// quitBlocked reports work that neither quitting nor closing may abandon.
func (r *Root) quitBlocked() bool {
	return r.confirming || r.job != nil && r.job.mutation || r.update.Status.Kind == core.UpdateInstalling
}

// onBeforeQuit admits every quit path (tray menu, tray popover, Cmd+Q)
// unless work is pending, which it cancels instead.
func (r *Root) onBeforeQuit(e *mygo.QuitEvent) {
	if r.closed {
		return
	}
	if r.quitBlocked() {
		e.PreventDefault()
		r.Cancel()
		return
	}
	r.quitting = true
}

// onMainClose hides the main window to the tray on an ordinary close. During
// an admitted quit it must let the window close: MyGo abandons the whole quit
// when any window refuses, so hiding here made every quit a no-op.
func (r *Root) onMainClose(e *mygo.CloseEvent, hide func()) {
	if r.closed {
		return
	}
	if r.tray != nil && !r.quitting {
		e.PreventDefault()
		hide()
		return
	}
	if r.quitBlocked() {
		e.PreventDefault()
		r.Cancel()
	}
}
