package main

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func dialogFixture(shell *appui.Dashboard, kind string) (func(*ui.Context), error) {
	open, dontAsk := true, false
	close := func() { open = false }
	switch kind {
	case "confirm", "release", "uninstall":
		s := appui.ConfirmationDialog{ID: "fixture", Title: core.NewText("确认清理", "Confirm cleanup"), Body: core.NewText("已选择 2 个项目，共 24 MB。", "2 items selected, 24 MB total."), Detail: core.NewText("此操作不可撤销。", "This operation cannot be undone.")}
		if kind == "release" {
			s.Kind = appui.ConfirmReleaseCache
			s.Title = core.NewText("退出应用后清理", "Quit apps and clean")
			s.Detail = core.NewText("Example Editor 正在占用缓存。退出后重新验证选中的目标。", "Example Editor is using these caches. Selected targets will be revalidated after quitting.")
		}
		if kind == "uninstall" {
			s.Kind = appui.ConfirmUninstall
			s.AppData = true
			s.Title = core.NewText("确认卸载 Example Editor", "Uninstall Example Editor")
			s.Detail = core.NewText("调用官方卸载程序，不会退回到直接删除。", "Uses the official uninstaller, never a deletion fallback.")
		}
		return func(c *ui.Context) {
			s.Open, s.Language = open, shell.Language
			s.Render(c, appui.ConfirmationDialogActions{Cancel: func(string) { close() }})
		}, nil
	case "permission":
		return func(c *ui.Context) {
			appui.PermissionDialog{Open: open, Language: shell.Language, DontAsk: dontAsk}.Render(c, appui.PermissionDialogActions{Later: close, SetDontAsk: func(on bool) { dontAsk = on }})
		}, nil
	case "update", "downloading", "ready", "error":
		status := core.UpdateStatus{Kind: core.UpdateAvailable, LatestVersion: "0.0.17", Notes: "QuickCleaner update\nPerformance improvements and verified cleanup fixes.", ReleaseURL: "https://github.com/meimingqi222/quick-cleaner/releases"}
		switch kind {
		case "downloading":
			status.Kind = core.UpdateDownloading
			status.Progress.Percent = 42
		case "ready":
			status.Kind = core.UpdateDownloaded
		case "error":
			status.Kind = core.UpdateError
			status.Message = "Checksum verification failed. The existing installation is unchanged."
		}
		return func(c *ui.Context) {
			appui.UpdateDialog{Open: open, Language: shell.Language, Update: appui.UpdateSnapshot{Status: status}}.Render(c, appui.UpdateDialogActions{Later: close})
		}, nil
	default:
		return nil, fmt.Errorf("unknown fixture dialog %q", kind)
	}
}
