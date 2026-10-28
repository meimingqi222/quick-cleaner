package app

import (
	"context"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/core"
	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

// 从废纸篓恢复安装后，残留的托盘行和打开的复核都要撤下：再提示清理一个
// 仍装着的应用，会诱导用户删掉活跃应用的数据。
func TestReinstalledAppWithdrawsLeftoverRowAndReview(t *testing.T) {
	r, q := testRoot(t)
	app := core.InstalledApp{ID: "com.coteditor.CotEditor", Name: "CotEditor"}
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: "p"}, core.ResidualCacheDir)
	r.trashLeftovers = []trayLeftover{
		{id: "com.coteditor.coteditor", app: app, result: core.ResidualScanResult{AppName: "CotEditor", Items: []core.ResidualItem{item}}},
		{id: "orphans", app: core.InstalledApp{Name: "Uninstalled leftovers"}, result: core.ResidualScanResult{Items: []core.ResidualItem{item}}},
	}
	r.trashSeen["com.coteditor.coteditor"] = true
	r.trashSeen["orphans"] = true
	r.showResiduals(core.ResidualScanResult{AppName: "CotEditor", Items: []core.ResidualItem{item}}, &app)
	cur := map[string]platform.TrashedApp{
		"com.coteditor.coteditor": {ID: "com.coteditor.coteditor", Name: "CotEditor", Path: "/Applications/CotEditor.app"},
	}
	r.withdrawReinstalled(cur)
	if r.trashSeen["com.coteditor.coteditor"] {
		t.Fatal("restored app still marked as reported")
	}
	if !r.trashSeen["orphans"] {
		t.Fatal("unrelated report lost its guard")
	}
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 1 || r.trashLeftovers[0].id != "orphans" {
		t.Fatalf("leftover rows after restore: %+v", r.trashLeftovers)
	}
	if r.apps.ResidualsOpen {
		t.Fatal("review of a reinstalled app stayed open")
	}
}

// 检测到卸载先挂出一行「扫描中」，扫描完成再替换成残留清单：提示不再
// 等文件遍历，用户几乎立刻看到反馈。
func TestUninstallReportSurfacesPendingRowThenFillsIt(t *testing.T) {
	r, q := testRoot(t)
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: "p"}, core.ResidualCacheDir)
	r.scanAppResiduals = func(context.Context, core.InstalledApp) (core.ResidualScanResult, error) {
		return core.ResidualScanResult{AppName: "CotEditor", Items: []core.ResidualItem{item}}, nil
	}
	r.bundleStillInstalled = func(context.Context, string) (bool, bool) { return false, true }
	r.reportUninstalled(platform.TrashedApp{ID: "com.coteditor.coteditor", Name: "CotEditor", Path: "/.Trash/CotEditor.app"})
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 1 || !r.trashLeftovers[0].pending || r.trashLeftovers[0].id != "com.coteditor.coteditor" {
		t.Fatalf("pending row missing: %+v", r.trashLeftovers)
	}
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 1 || r.trashLeftovers[0].pending || len(r.trashLeftovers[0].result.Items) != 1 {
		t.Fatalf("resolved row wrong: %+v", r.trashLeftovers)
	}
}

// 扫不到残留的 pending 行要撤掉，不留空壳。
func TestUninstallReportDropsPendingRowWithoutLeftovers(t *testing.T) {
	r, q := testRoot(t)
	r.scanAppResiduals = func(context.Context, core.InstalledApp) (core.ResidualScanResult, error) {
		return core.ResidualScanResult{}, nil
	}
	r.bundleStillInstalled = func(context.Context, string) (bool, bool) { return false, true }
	r.reportUninstalled(platform.TrashedApp{ID: "com.example.gone", Name: "Gone", Path: "/.Trash/Gone.app"})
	dispatchNext(t, q)
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 0 {
		t.Fatalf("empty report kept a row: %+v", r.trashLeftovers)
	}
}

// 另一个应用的复核不能跟着被误关。
func TestReinstalledAppLeavesOtherReviewOpen(t *testing.T) {
	r, q := testRoot(t)
	other := core.InstalledApp{ID: "com.example.Other", Name: "Other"}
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: "p"}, core.ResidualCacheDir)
	r.trashLeftovers = []trayLeftover{{id: "com.coteditor.coteditor", app: other, result: core.ResidualScanResult{Items: []core.ResidualItem{item}}}}
	r.trashSeen["com.coteditor.coteditor"] = true
	r.showResiduals(core.ResidualScanResult{AppName: "Other", Items: []core.ResidualItem{item}}, &other)
	cur := map[string]platform.TrashedApp{
		"com.coteditor.coteditor": {ID: "com.coteditor.coteditor", Name: "CotEditor", Path: "/Applications/CotEditor.app"},
	}
	r.withdrawReinstalled(cur)
	dispatchNext(t, q)
	if len(r.trashLeftovers) != 0 {
		t.Fatal("row survived the restore")
	}
	if !r.apps.ResidualsOpen {
		t.Fatal("unrelated review was closed")
	}
}
