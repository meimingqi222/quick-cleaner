package app

import (
	"errors"
	"fmt"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// wingetCodeMask matches the family of exit codes the winget CLI returns
// (0x8A15xxxx), which is how a package-manager refusal shows up here.
const wingetCodeMask = 0xffff0000
const wingetCodeFamily = 0x8a150000

// uninstallFailureNotice says what failed, that nothing was deleted, and where the
// user can go next. The uninstaller's own English text stays in the log and the
// history record; the page speaks the UI language and carries the exit code so the
// failure can still be reported to support.
func uninstallFailureNotice(err error) core.Text {
	var exited *core.UninstallerExitError
	if errors.As(err, &exited) {
		code := fmt.Sprintf("%#x", exited.Code)
		if exited.Code&wingetCodeMask == wingetCodeFamily {
			return core.NewText(
				fmt.Sprintf("官方卸载器（winget）异常退出（%s）；未删除任何文件。可在终端运行带 --force 的 winget uninstall 强制卸载。", code),
				fmt.Sprintf("The official uninstaller (winget) failed (%s); nothing was deleted. Run winget uninstall with --force in a terminal to override it.", code),
			)
		}
		return core.NewText(
			fmt.Sprintf("官方卸载器异常退出（%s）；未删除任何文件。请用应用自带的卸载入口重试。", code),
			fmt.Sprintf("The official uninstaller failed (%s); nothing was deleted. Retry from the app's own uninstaller.", code),
		)
	}
	// winget 指令不能指给「设置 → 应用」：它的 UninstallString 就是同一条
	// `winget uninstall` 命令，指过去只会再撞一次同样的拒绝。
	if errors.Is(err, core.ErrUninstallNotSettled) {
		return core.NewText(
			"卸载器已退出，但安装没有落定；文件可能仍被占用。请退出该应用后重试。",
			"The uninstaller exited but the installation did not settle; files may still be in use. Quit the app and retry.",
		)
	}
	var registered *core.StillRegisteredError
	if errors.As(err, &registered) {
		return core.NewText(
			fmt.Sprintf("卸载器已退出，但「%s」仍登记在应用列表里。请到「设置 → 应用」确认后再处理。", registered.Name),
			fmt.Sprintf("The uninstaller exited but \"%s\" is still registered. Check Settings → Apps before trying again.", registered.Name),
		)
	}
	// Unknown failures keep their own wording: inventing a translation for an error
	// nobody classified would hide what the platform actually said.
	return core.SameText(err.Error())
}
