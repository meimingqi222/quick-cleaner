package app

import (
	"runtime"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/assets"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// Run retains one root for the lifetime of the main window. Closing while a
// mutation is pending requests cancellation and waits for its truthful outcome.
func Run() error {
	mygo.App.SetName("QuickCleaner")
	mygo.App.SetVersion(Version)
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
		window = mygo.NewWindow(mygo.WindowOptions{Title: "QuickCleaner", Width: 1280, Height: 880, Content: ui.View(r.Render)})
		r.window = window
		window.OnClose(func(e *mygo.CloseEvent) {
			if !r.closed && r.tray != nil {
				e.PreventDefault()
				window.Hide()
				return
			}
			if !r.closed && (r.confirming || r.job != nil && r.job.mutation || r.update.Status.Kind == core.UpdateInstalling) {
				e.PreventDefault()
				r.Cancel()
			}
		})
		window.OnClosed(func() { r.Close(); mygo.App.Quit() })
		mygo.App.OnBeforeQuit(func(e *mygo.QuitEvent) {
			if !r.closed && (r.confirming || r.job != nil && r.job.mutation || r.update.Status.Kind == core.UpdateInstalling) {
				e.PreventDefault()
				r.Cancel()
			}
		})
		mygo.App.OnWillQuit(func(*mygo.QuitEvent) { r.Close() })
		mygo.App.OnActivate(func(bool) {
			if !r.closed {
				r.showMain()
			}
		})
		r.startUpdates()
		r.installMenu()
		if runtime.GOOS == "darwin" && !r.settings.MacOSFDADismissed {
			r.permission.Open = true
			r.checkPermission()
		}
		r.startTray()
		r.startMonitoring()
		r.loadVolumes()
	})
	return mygo.App.Run()
}
