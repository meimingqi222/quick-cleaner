package app

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

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
	// 产品自己会加 --force 重试，所以提示不该再让用户手动执行 --force
	// （那是在把程序该做的事推给人）；也不该指向「设置 → 应用」——那是同一条命令。
	if !strings.Contains(winget.Get(core.English), "0x8a150057") {
		t.Fatalf("english winget notice lost the code: %q", winget.Get(core.English))
	}
	for _, text := range []string{winget.Get(core.Chinese), winget.Get(core.English)} {
		if strings.Contains(text, "设置") || strings.Contains(text, "Settings") {
			t.Fatalf("winget notice points at the same failing command: %q", text)
		}
		if strings.Contains(text, "--force") {
			t.Fatalf("unforced notice tells the user to do what the product retries: %q", text)
		}
	}

	// 已经自动 --force 重试过仍失败时，要如实说明「已重试」，不能再提示去加 --force。
	forced := uninstallFailureNotice(&core.UninstallerExitError{Code: 0x8a150057, Forced: true})
	if !strings.Contains(forced.Get(core.Chinese), "已自动加 --force 重试") {
		t.Fatalf("forced notice does not admit the retry: %q", forced.Get(core.Chinese))
	}
	if !strings.Contains(forced.Get(core.Chinese), "0x8a150057") {
		t.Fatalf("forced notice lost the code: %q", forced.Get(core.Chinese))
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

func TestUninstallConfirmationDialogMatchesRustReference(t *testing.T) {
	r, _ := testRoot(t)
	loc := "/Applications/Example.app"
	uninstall := "example-uninstaller"
	r.installed = []core.InstalledApp{{
		ID:              "com.example.app",
		Name:            "Example",
		InstallLocation: &loc,
		UninstallString: &uninstall,
		EstimatedSize:   1024 * 1024 * 42,
	}}
	r.requestUninstall("com.example.app")
	if !r.confirming {
		t.Fatal("requestUninstall did not open confirmation dialog")
	}
	if r.confirmation.Kind != 2 /* ConfirmUninstall */ {
		t.Fatalf("unexpected kind: %v", r.confirmation.Kind)
	}
	titleZh := r.confirmation.Title.Get(core.Chinese)
	titleEn := r.confirmation.Title.Get(core.English)
	if titleZh != "确认卸载「Example」？" {
		t.Fatalf("titleZh does not match Rust reference: %q", titleZh)
	}
	if titleEn != "Uninstall \"Example\"?" {
		t.Fatalf("titleEn does not match Rust reference: %q", titleEn)
	}
	bodyZh := r.confirmation.Body.Get(core.Chinese)
	bodyEn := r.confirmation.Body.Get(core.English)
	if bodyZh == "" || !strings.Contains(bodyZh, "Example") {
		t.Fatalf("bodyZh empty or missing app name: %q", bodyZh)
	}
	if bodyEn == "" || !strings.Contains(bodyEn, "Example") {
		t.Fatalf("bodyEn empty or missing app name: %q", bodyEn)
	}
	detailZh := r.confirmation.Detail.Get(core.Chinese)
	if !strings.Contains(detailZh, "卸载成功后会列出关联配置与缓存") {
		t.Fatalf("detailZh missing residual explanation: %q", detailZh)
	}
}

func TestUninstallSuccessDoesNotLeaveResidualWarningBanner(t *testing.T) {
	r, q := testRoot(t)
	loc := filepath.Join(t.TempDir(), "Example.app")
	uninstall := "example-uninstaller"
	if err := os.Mkdir(loc, 0o755); err != nil {
		t.Fatal(err)
	}
	r.installed = []core.InstalledApp{{
		ID:              "com.example.app",
		Name:            "Example",
		InstallLocation: &loc,
		UninstallString: &uninstall,
	}}
	r.uninstallApp = func(core.InstalledApp) error { return nil }
	r.scanAppResiduals = func(context.Context, core.InstalledApp) (core.ResidualScanResult, error) {
		return core.ResidualScanResult{AppName: "Example"}, nil
	}
	r.requestUninstall("com.example.app")
	if !r.confirming {
		t.Fatal("confirmation did not open")
	}
	r.resolveConfirmation(r.confirmation.ID, true)
	if r.job == nil {
		t.Fatalf("uninstall job never started (confirming=%v notice=%q)", r.confirming, r.apps.Notice.Get(core.Chinese))
	}
	// dispatch progress and completion
	dispatchNext(t, q) // phase: removing
	dispatchNext(t, q) // phase: verifying
	dispatchNext(t, q) // completion
	if r.apps.Notice.Get(core.Chinese) != "" {
		t.Fatalf("uninstall success left a warning banner on AppsPage: %q", r.apps.Notice.Get(core.Chinese))
	}
	if !strings.Contains(r.dashboard.StatusText, "Example") || (!strings.Contains(r.dashboard.StatusText, "uninstaller finished") && !strings.Contains(r.dashboard.StatusText, "卸载程序已完成")) {
		t.Fatalf("status text did not report uninstaller finished: %q", r.dashboard.StatusText)
	}
}

func TestResidualCleanClearsNotice(t *testing.T) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "windows" {
		t.Skip("native residual verification is only available on macOS and Windows")
	}
	r, q := testRoot(t)
	app := core.InstalledApp{ID: "example", Name: "Example"}
	gone := filepath.Join(t.TempDir(), "already-gone")
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: gone}, core.ResidualCacheDir)
	r.showResiduals(core.ResidualScanResult{AppName: app.Name, Items: []core.ResidualItem{item}}, &app)
	r.residualAuthority = func(context.Context, core.InstalledApp, []core.ResidualItem) bool { return true }
	r.apps.Notice = core.NewText("历史提示", "historical notice")
	r.requestResidualClean()
	dispatchNext(t, q)
	if r.apps.Notice.Get(core.Chinese) != "" {
		t.Fatalf("successful residual clean did not clear notice: %q", r.apps.Notice.Get(core.Chinese))
	}
}

// 卸载成功直接从列表删行，不再重扫清单：残留的快捷方式/缓存目录会让
// ListInstalledApps 把已卸载的应用重新「发现」回来，行必须靠移除消失。
func TestSuccessfulUninstallDropsTheRowWithoutRescan(t *testing.T) {
	r, q := testRoot(t)
	loc := filepath.Join(t.TempDir(), "Example")
	uninstall := "example-uninstaller"
	if err := os.MkdirAll(loc, 0o755); err != nil {
		t.Fatal(err)
	}
	r.installed = []core.InstalledApp{
		{ID: "com.example.app", Name: "Example", InstallLocation: &loc, UninstallString: &uninstall},
		{ID: "dev.other.app", Name: "Other"},
	}
	r.rebuildApps()
	r.uninstallApp = func(core.InstalledApp) error { return nil }
	r.scanAppResiduals = func(context.Context, core.InstalledApp) (core.ResidualScanResult, error) {
		return core.ResidualScanResult{}, nil
	}
	r.requestUninstall("com.example.app")
	if !r.confirming {
		t.Fatal("confirmation did not open")
	}
	r.resolveConfirmation(r.confirmation.ID, true)
	if r.job == nil {
		t.Fatalf("uninstall job never started (confirming=%v notice=%q)", r.confirming, r.apps.Notice.Get(core.Chinese))
	}
	// 残留复核阶段的相位更新也要吐掉，最终 finish 才结算结果。
	for i := 0; i < 8 && !r.apps.ResidualsOpen; i++ {
		dispatchNext(t, q)
	}
	deadline := time.After(5 * time.Second)
	for !r.apps.ResidualsOpen {
		select {
		case fn := <-q:
			fn()
		case <-deadline:
			t.Fatal("residual review never opened")
		}
	}
	for _, inst := range r.installed {
		if inst.ID == "com.example.app" {
			t.Fatal("uninstalled app survived in the list")
		}
	}
	if len(r.installed) != 1 || r.dashboard.AppCount != 1 {
		t.Fatalf("list = %v apps count = %d", r.installed, r.dashboard.AppCount)
	}
}
