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
					platform.HideFDADrop()
					r.permission.Open = false
					r.dashboard.FullDiskAccess = true
					r.message("已确认完全磁盘访问权限。", "Full Disk Access confirmed.")
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
				r.permission.Status = core.NewText("已确认完全磁盘访问权限。", "Full Disk Access confirmed.")
				r.permission.Open = false
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

func (r *Root) checkUpdates() {
	if r.updates == nil {
		r.unavailable()
		return
	}
	r.updates.Check(true)
}
func (r *Root) requestInstallUpdate() {
	if r.updates == nil || r.updates.Snapshot().Status.Kind != core.UpdateDownloaded {
		return
	}
	if r.busy() {
		r.message("请先停止当前任务，再安装更新。", "Stop the current task before installing an update.")
		return
	}
	r.askKind(view.ConfirmInstallUpdate, r.tr("安装更新并重新启动？", "Install update and restart?"), r.tr("更新安装器将替换应用并重新启动。", "The update installer will replace the application and restart it."), func() {
		r.updates.Install(false, true)
		r.update = r.updates.Snapshot()
	})
}

func (r *Root) installMenu() {
	if r.window == nil {
		return
	}
	item := func(zh, en string, fn func()) *mygo.MenuItem {
		return &mygo.MenuItem{Label: r.tr(zh, en), Click: func(*mygo.MenuItem, *mygo.Window) { fn() }}
	}
	mygo.App.SetMenu(mygo.NewMenu([]*mygo.MenuItem{{Label: "QuickCleaner", Submenu: []*mygo.MenuItem{
		item("扫描当前页面", "Scan current page", r.Scan), item("停止当前任务", "Stop current task", r.Cancel),
		{Label: r.tr("删除到回收站", "Delete to Recycle Bin / Trash"), Type: mygo.MenuItemCheckbox, Checked: r.settings.DeleteToRecycleBin, Click: func(i *mygo.MenuItem, _ *mygo.Window) {
			if r.busy() {
				i.SetChecked(r.settings.DeleteToRecycleBin)
				return
			}
			r.settings.DeleteToRecycleBin = i.Checked
			r.disk.ToRecycleBin = i.Checked
			r.save(r.settings)
		}},
		{Label: r.tr("自动检查更新", "Automatically check for updates"), Type: mygo.MenuItemCheckbox, Checked: r.settings.AutoCheckUpdates, Click: func(i *mygo.MenuItem, _ *mygo.Window) { r.updates.SetAutoCheck(i.Checked) }},
		item("检查更新", "Check for updates", r.checkUpdates), item("权限", "Permissions", r.permissions), mygo.Separator(), {Role: mygo.RoleQuit},
	}}}))
	if r.tray != nil && runtime.GOOS == "linux" {
		r.tray.SetMenu(r.trayMenu())
	}
}
