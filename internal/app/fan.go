package app

import (
	"errors"
	"fmt"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func (r *Root) requestFanMode(mode core.FanMode) {
	r.beginFan(mode, false)
}

// beginFan keeps a percent mode applied until another fan action wins the
// generation. It does not take the product job lock, so a scan can run too.
// The install confirmation is shown only when the helper is missing.
func (r *Root) beginFan(mode core.FanMode, allowInstall bool) {
	if !platform.FanControlSupported() || !mode.IsSupported() {
		r.unavailable()
		return
	}
	if r.status.FanApplying || (r.status.FanMode == mode && !r.status.FanStale && !allowInstall) {
		return
	}
	previous := r.status.FanMode
	generation := r.fanGeneration.Add(1)
	r.status.FanMode = mode
	r.status.FanApplying = true
	r.status.FanStale = false
	r.changed()
	prompt := r.tr(
		"QuickCleaner 需要安装或更新风扇控制组件。授权后切换风扇档位不再需要密码。",
		"QuickCleaner needs to install or update its fan control component. After this, switching fan modes never needs a password.",
	)
	go func() {
		announced := false
		useHelper := false
		for {
			if r.fanGeneration.Load() != generation {
				return
			}
			err := applyFanOnce(mode, &useHelper, allowInstall, announced, prompt)
			if err != nil {
				restored := restoreFanAuto()
				needsConsent := fanKind(err) == core.FanNotInstalled
				r.publish(func() {
					if r.fanGeneration.Load() != generation {
						return
					}
					if restored {
						r.status.FanMode = core.FanMode{Kind: core.FanAuto}
					} else {
						r.status.FanMode = previous
					}
					r.status.FanApplying = false
					r.status.FanStale = !restored && previous.Kind == core.FanPercent
					if needsConsent {
						r.message(
							"macOS 固件只允许 root 写风扇转速。安装一个后台组件后，以后切换档位不再需要输入密码。",
							"macOS firmware only lets root write fan speeds. After installing a background component, switching modes never asks for a password again.",
						)
						r.askKind(view.ConfirmInstallHelper,
							r.tr("需要安装风扇控制组件", "Fan control component required"),
							r.tr(
								"macOS 固件只允许 root 写风扇转速。安装一个后台组件后，以后切换档位不再需要输入密码。\n\n接下来会弹一次系统密码框——只有这一次。组件只接受「自动 / 降温 / 全速」三条指令；应用退出时风扇立即交还系统调速。随时可在风扇卡片上移除。",
								"macOS firmware only lets root write fan speeds. After installing a background component, switching modes never asks for a password again.\n\nmacOS will ask for your password once — only this once. The component accepts only three commands (auto / boost / full), and hands the fans back to the system the moment the app quits. Removable any time from the fan card.",
							),
							func() { r.beginFan(mode, true) },
						)
						return
					}
					r.fanFailure(err)
				})
				return
			}
			if !announced {
				announced = true
				installed := platform.FanHelperInstalled()
				zh, en := fanApplied(mode)
				r.publish(func() {
					if r.fanGeneration.Load() != generation {
						return
					}
					r.status.FanApplying = false
					r.status.FanStale = false
					r.status.FanHelperInstalled = installed
					r.status.FanMode = mode
					r.message(zh, en)
				})
			}
			if mode.Kind == core.FanAuto {
				return
			}
			time.Sleep(3 * time.Second)
		}
	}()
}

func applyFanOnce(mode core.FanMode, useHelper *bool, allowInstall, announced bool, prompt string) error {
	if !*useHelper {
		err := platform.SetFanMode(mode)
		if fanKind(err) == core.FanNeedsRoot {
			*useHelper = true
		} else {
			return err
		}
	}
	err := platform.ElevatedFanControl(mode)
	kind := fanKind(err)
	if !((kind == core.FanNeedsUpgrade && !announced) || (kind == core.FanNotInstalled && allowInstall)) {
		return err
	}
	if err = platform.InstallFanHelper(prompt); err != nil {
		return err
	}
	return platform.ElevatedFanControl(mode)
}

func restoreFanAuto() bool {
	auto := core.FanMode{Kind: core.FanAuto}
	err := platform.SetFanMode(auto)
	if err == nil {
		return true
	}
	if fanKind(err) != core.FanNeedsRoot {
		return false
	}
	return platform.ElevatedFanControl(auto) == nil
}

func fanKind(err error) core.FanErrorKind {
	var fanErr core.FanError
	if errors.As(err, &fanErr) {
		return fanErr.Kind
	}
	return core.FanOther
}

func (r *Root) fanFailure(err error) {
	if fanKind(err) == core.FanCanceled {
		r.message("已取消管理员授权，风扇档位未更改", "Admin authorization canceled; fan mode unchanged")
		return
	}
	text := ""
	if err != nil {
		text = err.Error()
	}
	r.message("风扇控制失败："+text, "Fan control failed: "+text)
}

func fanApplied(mode core.FanMode) (string, string) {
	switch {
	case mode.Kind == core.FanAuto:
		return "风扇已恢复系统自动调速", "Fan control returned to system"
	case mode.Kind == core.FanPercent && mode.Percent == 60:
		return "降温模式已开启，转速将随温度升高", "Cooling enabled; fan speed will rise with temperature"
	default:
		p := fmt.Sprint(mode.Percent)
		return "风扇已锁定在最大转速的 " + p + "%", "Fans locked at " + p + "% of max speed"
	}
}

func (r *Root) requestRemoveFanHelper() {
	if !platform.FanControlSupported() || r.status.FanApplying {
		return
	}
	prompt := r.tr(
		"QuickCleaner 需要移除风扇控制组件，风扇将交还系统调速。",
		"QuickCleaner needs to remove its fan control component; the fans will be handed back to the system.",
	)
	r.askKind(view.ConfirmUninstall,
		r.tr("移除风扇控制组件", "Remove fan control component"),
		r.tr("将恢复自动模式并移除特权助手。系统会要求授权。", "Restores automatic mode and removes the privileged helper. System authorization is required."),
		func() {
			generation := r.fanGeneration.Add(1)
			r.status.FanApplying = true
			r.changed()
			go func() {
				_ = platform.SetFanMode(core.FanMode{Kind: core.FanAuto})
				err := platform.UninstallFanHelper(prompt)
				installed := platform.FanHelperInstalled()
				r.publish(func() {
					if r.fanGeneration.Load() != generation {
						return
					}
					r.status.FanApplying = false
					r.status.FanHelperInstalled = installed
					r.status.FanMode = core.FanMode{Kind: core.FanAuto}
					r.status.FanStale = false
					if err != nil {
						r.fanFailure(err)
						return
					}
					r.message("风扇控制组件已移除，风扇已交还系统调速", "Fan control component removed; fans returned to the system")
				})
			}()
		})
}
