package app

import (
	"errors"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
)

// 卸载失败要说出「发生了什么、有没有删东西、下一步去哪」，并把退出码带出来；
// 官方卸载器的英文原文只留在日志与历史记录里。
func TestUninstallFailureNoticeExplainsAndKeepsTheCode(t *testing.T) {
	winget := uninstallFailureNotice(&core.UninstallerExitError{Code: 0x8a150057})
	if !strings.Contains(winget.Get(core.Chinese), "winget") || !strings.Contains(winget.Get(core.Chinese), "0x8a150057") {
		t.Fatalf("winget notice lost its code or hint: %q", winget.Get(core.Chinese))
	}
	if !strings.Contains(winget.Get(core.Chinese), "未删除") {
		t.Fatalf("winget notice does not say nothing was deleted: %q", winget.Get(core.Chinese))
	}
	if !strings.Contains(winget.Get(core.English), "0x8a150057") || !strings.Contains(winget.Get(core.English), "Settings") {
		t.Fatalf("english winget notice = %q", winget.Get(core.English))
	}

	other := uninstallFailureNotice(&core.UninstallerExitError{Code: 0x2})
	if strings.Contains(other.Get(core.Chinese), "winget") {
		t.Fatalf("non-winget exit code claimed winget: %q", other.Get(core.Chinese))
	}
	if !strings.Contains(other.Get(core.Chinese), "0x2") || !strings.Contains(other.Get(core.Chinese), "未删除") {
		t.Fatalf("generic exit notice = %q", other.Get(core.Chinese))
	}

	settled := uninstallFailureNotice(core.ErrUninstallNotSettled)
	if !strings.Contains(settled.Get(core.Chinese), "占用") {
		t.Fatalf("not-settled notice = %q", settled.Get(core.Chinese))
	}

	registered := uninstallFailureNotice(&core.StillRegisteredError{Name: "Copilot CLI"})
	if !strings.Contains(registered.Get(core.Chinese), "Copilot CLI") {
		t.Fatalf("registered notice lost the name: %q", registered.Get(core.Chinese))
	}

	// 没分类的失败保留原文：给它编一句翻译会掩盖平台真正说的话。
	unknown := uninstallFailureNotice(errors.New("boom"))
	if unknown.Get(core.Chinese) != "boom" || unknown.Get(core.English) != "boom" {
		t.Fatalf("unknown error was rewritten: %q / %q", unknown.Get(core.Chinese), unknown.Get(core.English))
	}
}
