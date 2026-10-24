package app

import (
	"runtime"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) renderDialogs(c *ui.Context) {
	// Destructive confirmations suspend update/FDA prompts, never vice versa.
	if r.confirming {
		r.confirmation.Language = r.settings.Language
		r.confirmation.Render(c, view.ConfirmationDialogActions{Accept: func(id string) { r.resolveConfirmation(id, true) }, Cancel: func(id string) { r.resolveConfirmation(id, false) }})
		return
	}
	if r.permission.Open {
		r.permission.Language = r.settings.Language
		r.permission.Render(c, view.PermissionDialogActions{Later: func() { r.permission.Open = false; platform.HideFDADrop() }, CheckStatus: r.checkPermission, OpenSettings: r.openFDASettings, SetDontAsk: func(on bool) { r.permission.DontAsk = on; r.settings.MacOSFDADismissed = on; r.save(r.settings) }})
		return
	}
	if r.updates != nil && r.update.ShowDialog {
		view.UpdateDialog{Open: true, Language: r.settings.Language, Update: r.update}.Render(c, view.UpdateDialogActions{Later: r.updates.DismissDialog, SkipVersion: r.updates.SkipVersion, Download: r.updates.StartDownload, Retry: r.updates.Retry, Install: r.requestInstallUpdate, OpenRelease: func(url string) {
			go func() { err := mygo.Shell.OpenExternal(url); r.publish(func() { r.failure(err) }) }()
		}})
	}
}

func (r *Root) openFDASettings() {
	go func() {
		err := platform.OpenFullDiskAccessSettings()
		r.publish(func() { r.failure(err) })
	}()
	bundle, ok := platform.EnclosingAppBundle()
	if !ok {
		return
	}
	r.generation++
	gen := r.generation
	go func() {
		text := platform.FDADropText{}
		for tick := 0; tick < 2000; tick++ {
			time.Sleep(150 * time.Millisecond)
			var stop bool
			r.publish(func() {
				if gen != r.generation || !r.permission.Open {
					platform.HideFDADrop()
					stop = true
					return
				}
				if tick%4 == 0 && platform.HasFullDiskAccess() {
					r.grantFullDiskAccess()
					stop = true
					return
				}
				if !platform.FDADropVisible() || tick < 80 {
					zh := r.settings.Language != core.English
					text = platform.FDADropText{AppName: "QuickCleaner"}
					if zh {
						text.Line1, text.Line2 = "把左边的图标拖进上方的列表", "松手即完成授权，无需再点开关"
					} else {
						text.Line1, text.Line2 = "Drag the icon on the left into the list above", "Drop it there to grant access — no switch to flip"
					}
					// AppKit only from the main-thread publisher, matching Rust's update callback.
					platform.ShowFDADrop(bundle, text, tick >= 20)
				}
			})
			if stop {
				return
			}
		}
		r.publish(func() { platform.HideFDADrop() })
	}()
}

func (r *Root) checkPermission() {
	if r.permission.Busy {
		return
	}
	r.permission.Busy = true
	go func() {
		access := platform.HasFullDiskAccess()
		r.publish(func() {
			r.permission.Busy = false
			r.dashboard.FullDiskAccess = access
			if access {
				r.grantFullDiskAccess()
			} else {
				r.permission.Status = core.NewText("尚未确认权限。设置后请再次检查。", "Access is not confirmed. Check again after changing settings.")
			}
		})
	}()
}

func (r *Root) startUpdates() {
	r.updates = view.NewUpdateController(Version, view.UpdatePreferences{AutoCheck: r.settings.AutoCheckUpdates, SkippedVersion: r.settings.SkippedUpdateVersion, LastCheckAt: r.settings.LastUpdateCheckAt}, nil)
	u := r.updates
	ctx := r.ctx
	go func() {
		for {
			select {
			case <-ctx.Done():
				return
			case <-u.Changes():
				r.publish(func() {
					snapshot := u.Snapshot()
					r.update = snapshot
					r.refreshUpdateChrome()
					// The controller mutex must never be held across a synchronous
					// main-thread dispatch or a settings save.
					p := snapshot.Preferences
					preferencesChanged := r.settings.AutoCheckUpdates != p.AutoCheck || r.settings.SkippedUpdateVersion != p.SkippedVersion || r.settings.LastUpdateCheckAt != p.LastCheckAt
					r.settings.AutoCheckUpdates = p.AutoCheck
					r.settings.SkippedUpdateVersion = p.SkippedVersion
					r.settings.LastUpdateCheckAt = p.LastCheckAt
					if preferencesChanged {
						r.save(r.settings)
						r.installMenu()
					}
					if snapshot.ManualCheckError != "" {
						r.dashboard.StatusText = snapshot.ManualCheckError
					}
					if snapshot.Status.Kind == core.UpdateNotAvailable {
						r.message("当前已是最新版本。", "You are up to date.")
					}
					if snapshot.ReadyToQuit {
						r.Close()
						mygo.App.Quit()
					}
				})
			}
		}
	}()
	u.StartScheduler()
}

func (r *Root) grantFullDiskAccess() {
	platform.HideFDADrop()
	r.permission.Status = core.NewText("已确认完全磁盘访问权限。", "Full Disk Access confirmed.")
	r.permission.Open = false
	r.dashboard.FullDiskAccess = true
	r.message("已确认完全磁盘访问权限。", "Full Disk Access confirmed.")
	// A grant rescans even when an earlier scan already finished. A scan or
	// clean that is already running is left alone.
	if !r.busy() {
		r.scanJunk()
	}
}

func (r *Root) refreshUpdateChrome() {
	r.dashboard.UpdateLabel, r.dashboard.UpdateAttention = view.UpdateSidebarLabel(r.settings.Language, Version, r.update.Status, r.update.Checking)
}

func (r *Root) onVersionClick() {
	switch view.VersionClickKind(r.update.Status, r.update.Checking, platform.IsPackagedInstall()) {
	case view.VersionClickInstall:
		r.requestInstallUpdate()
	case view.VersionClickOpen:
		if r.updates != nil {
			r.updates.OpenDialog()
		}
	case view.VersionClickCheck:
		r.checkUpdates()
		r.message("正在检查…", "Checking…")
	}
}

func (r *Root) checkUpdates() {
	if r.updates == nil {
		r.unavailable()
		return
	}
	r.updates.Check(true)
}
func (r *Root) requestInstallUpdate() {
	if r.updates == nil || r.closed {
		return
	}
	// Idle: install immediately. A running scan or clean confirms once, because
	// the helper quits this process. busy() is that task, not a second prompt.
	if r.updates.Install(r.busy(), false) {
		r.confirmInstallOverTask()
		return
	}
	r.update = r.updates.Snapshot()
	r.refreshUpdateChrome()
	r.changed()
}

// confirmInstallOverTask opens while r.job is still current. It must not bump
// generation: scan completion is dropped when generation changes.
func (r *Root) confirmInstallOverTask() {
	if r.closed || r.confirming {
		return
	}
	r.confirming = true
	r.confirmation = view.ConfirmationDialog{
		Open: true, Language: r.settings.Language, ID: "update-install", Kind: view.ConfirmInstallUpdate,
		Title: core.SameText(r.tr("打断当前任务并安装更新？", "Interrupt current work and install the update?")),
		Body:  core.SameText(r.tr("扫描或清理仍在进行。安装更新会退出应用并中断这些任务。", "A scan or clean is still running. Installing will quit the app and interrupt that work.")),
	}
	r.accept = func() {
		if r.updates == nil {
			return
		}
		r.updates.Install(true, true)
		r.update = r.updates.Snapshot()
		r.refreshUpdateChrome()
	}
	r.changed()
}

func (r *Root) installMenu() {
	if r.window == nil {
		return
	}
	item := func(zh, en string, fn func()) *mygo.MenuItem {
		return &mygo.MenuItem{Label: r.tr(zh, en), Click: func(*mygo.MenuItem, *mygo.Window) { fn() }}
	}
	// The app menu is the macOS idiom (系统菜单栏 + Cmd+Q). Windows and Linux draw an
	// application menu inside the window, which repeated the native caption as a second
	// title bar; those platforms reach every action from the page or the tray instead.
	if runtime.GOOS == "darwin" {
		// The recycle-bin and auto-check switches live on the Disk Lens page and in the
		// sidebar settings area; keeping them here too would be a second source of truth.
		mygo.App.SetMenu(mygo.NewMenu([]*mygo.MenuItem{{Label: "QuickCleaner", Submenu: []*mygo.MenuItem{
			item("扫描当前页面", "Scan current page", r.Scan), item("停止当前任务", "Stop current task", r.Cancel),
			item("检查更新", "Check for updates", r.checkUpdates), item("权限", "Permissions", r.permissions),
			mygo.Separator(), {Role: mygo.RoleQuit},
		}}}))
	}
	if r.tray != nil && runtime.GOOS == "linux" {
		r.tray.SetMenu(r.trayMenu())
	}
}
