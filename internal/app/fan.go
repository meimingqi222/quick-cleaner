package app

import (
	"context"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) requestFanMode(mode core.FanMode) {
	if r.busy() {
		return
	}
	if !platform.FanControlSupported() || !mode.IsSupported() {
		r.unavailable()
		return
	}
	installed := r.status.FanHelperInstalled
	title := r.tr("更改风扇模式？", "Change fan mode?")
	detail := r.tr("通过会话所属的特权助手应用受支持的风扇模式；断开连接后恢复自动模式。", "Applies a supported fan mode through the session-owned privileged helper; disconnect restores automatic mode.")
	if !installed {
		title = r.tr("安装风扇助手并应用模式？", "Install fan helper and apply mode?")
		detail += "\n\n" + r.tr("系统将要求管理员授权。", "The system will request administrator authorization.")
	}
	prompt := r.tr("QuickCleaner 需要安装风扇控制助手。", "QuickCleaner needs to install the fan control helper.")
	r.askKind(view.ConfirmInstallHelper, title, detail, func() {
		r.status.FanApplying = true
		r.start(true, nil, func(ctx context.Context) func() {
			var err error
			if !installed {
				err = platform.InstallFanHelper(prompt)
			}
			if err == nil && ctx.Err() != nil {
				err = ctx.Err()
			}
			if err == nil {
				err = platform.ElevatedFanControl(mode)
			}
			nowInstalled := platform.FanHelperInstalled()
			return func() {
				r.status.FanApplying = false
				r.status.FanHelperInstalled = nowInstalled
				if err != nil {
					r.failure(err)
					return
				}
				r.status.FanMode = mode
				r.message("风扇模式已应用。", "Fan mode applied.")
			}
		})
	})
}

func (r *Root) requestRemoveFanHelper() {
	if r.busy() || !platform.FanControlSupported() {
		return
	}
	prompt := r.tr("QuickCleaner 需要移除风扇控制助手。", "QuickCleaner needs to remove the fan control helper.")
	r.askKind(view.ConfirmUninstall, r.tr("移除风扇控制助手？", "Remove fan control helper?"), r.tr("将恢复自动模式并移除特权助手。系统会要求授权。", "Restores automatic mode and removes the privileged helper. System authorization is required."), func() {
		r.status.FanApplying = true
		r.start(true, nil, func(context.Context) func() {
			err := platform.UninstallFanHelper(prompt)
			installed := platform.FanHelperInstalled()
			return func() {
				r.status.FanApplying = false
				r.status.FanHelperInstalled = installed
				if err != nil {
					r.failure(err)
					return
				}
				r.status.FanMode = core.FanMode{Kind: core.FanAuto}
				r.message("风扇助手已移除。", "Fan helper removed.")
			}
		})
	})
}
