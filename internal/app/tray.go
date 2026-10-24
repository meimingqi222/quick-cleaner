package app

import (
	"context"
	"runtime"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/assets"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) trayMenu() *mygo.Menu {
	return mygo.NewMenu([]*mygo.MenuItem{
		{Label: "QuickCleaner", Click: func(*mygo.MenuItem, *mygo.Window) { r.showMain() }},
		{Label: r.tr("概览", "Overview"), Click: func(*mygo.MenuItem, *mygo.Window) { r.togglePopover() }},
		{Label: r.tr("扫描垃圾", "Scan junk"), Click: func(*mygo.MenuItem, *mygo.Window) { r.routeOverview(view.OverviewAction{Kind: view.OverviewScanJunk}) }},
		{Role: mygo.RoleQuit},
	})
}

func (r *Root) startTray() {
	opts := mygo.TrayOptions{ID: "quick-cleaner", Icon: assets.IconPNG, ToolTip: "QuickCleaner"}
	if runtime.GOOS == "linux" {
		opts.Menu = r.trayMenu()
	}
	tray, err := mygo.NewTray(opts)
	if err != nil {
		r.failure(err)
		return
	}
	r.tray = tray
	tray.OnClick(r.togglePopover)
	tray.OnRightClick(func() { r.trayMenu().Popup(nil) })
}

func (r *Root) showMain() {
	if r.popover != nil {
		r.popover.Close()
	}
	if r.window != nil && !r.window.IsDestroyed() {
		mygo.App.Show()
		r.window.Show()
		r.window.Focus()
	}
}

func (r *Root) togglePopover() {
	if r.closed || r.tray == nil {
		return
	}
	if r.popover != nil {
		r.popover.Close()
		return
	}
	controller := view.NewTrayOverviewController()
	ctx, cancel := context.WithCancel(r.ctx)
	r.trayController = controller
	r.trayCancel = cancel
	snapshot := view.TrayPopover{Language: r.settings.Language}
	var window *mygo.Window
	window = mygo.NewFlyout(mygo.FlyoutOptions{Tray: r.tray, Placement: mygo.PlacementBottom, Width: 360, Height: 560, Focusable: true, Shadow: true, Content: ui.View(func(c *ui.Context) {
		snapshot.Language = r.settings.Language
		snapshot.Render(c, view.TrayPopoverActions{Dismiss: func() { window.Close() }, Emit: func(a view.OverviewAction) {
			if a.DismissesPopover() {
				window.Close()
			}
			controller.Handle(a, r.routeOverview)
		}})
	})})
	r.popover = window
	window.OnClosed(func() {
		cancel()
		controller.Close()
		if r.popover == window {
			r.popover = nil
			r.trayController = nil
			r.trayCancel = nil
		}
	})
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case <-controller.Changes():
				state := controller.Snapshot()
				r.publish(func() {
					if r.popover != window || ctx.Err() != nil {
						return
					}
					snapshot.Snapshot = state
					window.Invalidate()
				})
			}
		}
	}()
	controller.Start()
}

func (r *Root) routeOverview(a view.OverviewAction) {
	switch a.Kind {
	case view.OverviewQuit:
		mygo.App.Quit()
	case view.OverviewOpenMain:
		r.showMain()
	case view.OverviewScanJunk:
		r.showMain()
		r.Navigate(view.Junk)
		r.scanJunk()
	case view.OverviewOpenView:
		r.showMain()
		r.Navigate(a.View)
	case view.OverviewAnalyzeDisk:
		r.showMain()
		r.Navigate(view.Disk)
		if !r.busy() {
			for _, v := range r.disk.Volumes {
				if v.ID.Display() == a.Path {
					r.diskActions().SwitchVolume(v.ID)
					return
				}
			}
			r.message("所选卷已不可用。", "The selected volume is no longer available.")
		}
	}
}
