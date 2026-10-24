package ui_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func TestUpdateSidebarLabelAndClick(t *testing.T) {
	label, attention := appui.UpdateSidebarLabel(appui.English, "0.0.16", core.UpdateStatus{Kind: core.UpdateDownloaded, LatestVersion: "0.0.17"}, false)
	if label != "Restart to install 0.0.17" || !attention {
		t.Fatalf("downloaded label = %q attention %v", label, attention)
	}
	label, attention = appui.UpdateSidebarLabel(appui.Chinese, "0.0.16", core.UpdateStatus{Kind: core.UpdateChecking}, false)
	if label != "正在检查…" || attention {
		t.Fatalf("checking label = %q attention %v", label, attention)
	}
	if appui.VersionClickKind(core.UpdateStatus{Kind: core.UpdateDownloaded}, false, false) != appui.VersionClickInstall {
		t.Fatal("downloaded click")
	}
	if appui.VersionClickKind(core.UpdateStatus{Kind: core.UpdateAvailable, LatestVersion: "9"}, false, true) != appui.VersionClickOpen {
		t.Fatal("available click")
	}
	if appui.VersionClickKind(core.UpdateStatus{Kind: core.UpdateIdle}, true, true) != appui.VersionClickIgnore {
		t.Fatal("checking click")
	}
	if appui.VersionClickKind(core.UpdateStatus{Kind: core.UpdateIdle}, false, false) != appui.VersionClickIgnore {
		t.Fatal("unpackaged click")
	}
	if appui.VersionClickKind(core.UpdateStatus{Kind: core.UpdateIdle}, false, true) != appui.VersionClickCheck {
		t.Fatal("idle packaged click")
	}
	clicks := 0
	s := appui.Dashboard{Language: appui.English, Version: "v0.0.16", UpdateLabel: label, UpdateAttention: true, Cleanable: "0 B", Freed: "0 B"}
	s.UpdateLabel = "Restart to install 0.0.17"
	tt := ui.NewTester(func(c *ui.Context) { s.Render(c, appui.DashboardActions{CheckUpdates: func() { clicks++ }}) }, 1280, 880)
	if err := tt.Click("Restart to install 0.0.17"); err != nil {
		t.Fatal(err)
	}
	if clicks != 1 {
		t.Fatalf("version clicks = %d", clicks)
	}
}

func TestDashboardActions(t *testing.T) {
	for _, tc := range []struct {
		name, circle string
		snapshot     appui.Dashboard
		wantScans    int
		wantRoutes   int
		wantReview   bool
	}{
		{name: "idle", circle: "Click to Start Smart Scan", wantScans: 2},
		{name: "empty", circle: "No Junk Found", snapshot: appui.Dashboard{Scanned: true}, wantScans: 1, wantRoutes: 1},
		{name: "found", circle: "Cleanable Found", snapshot: appui.Dashboard{Scanned: true, HasJunk: true}, wantScans: 1, wantRoutes: 1, wantReview: true},
		{name: "scanning", circle: "Checking system caches and dev leftovers", snapshot: appui.Dashboard{Scanning: true}},
		{name: "cleaning", circle: "Permanently removing selected items", snapshot: appui.Dashboard{Scanned: true, HasJunk: true, Cleaning: true}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := tc.snapshot
			s.Language, s.Cleanable, s.Freed = appui.English, "1.25 GB", "0 B"
			scans, routes := 0, 0
			actions := appui.DashboardActions{
				Scan: func() { scans++ },
				Navigate: func(v appui.View) {
					if v != appui.Junk {
						t.Errorf("circle/review route = %v, want Junk", v)
					}
					routes++
				},
			}
			tt := ui.NewTester(func(c *ui.Context) { s.Render(c, actions) }, 1280, 880)
			if err := tt.Click(tc.circle); err != nil {
				t.Fatal(err)
			}
			rescan := "Rescan"
			if s.Scanning || s.Cleaning {
				rescan = "Scanning…"
			}
			if err := tt.Click(rescan); err != nil {
				t.Fatal(err)
			}
			if scans != tc.wantScans || routes != tc.wantRoutes {
				t.Fatalf("scans/routes = %d/%d, want %d/%d", scans, routes, tc.wantScans, tc.wantRoutes)
			}
			if got := tt.HasText("Review & Clean Junk"); got != tc.wantReview {
				t.Fatalf("review visible = %v, want %v", got, tc.wantReview)
			}
			if tc.wantReview {
				if err := tt.Click("Review & Clean Junk"); err != nil {
					t.Fatal(err)
				}
				if routes != tc.wantRoutes+1 || scans != tc.wantScans {
					t.Fatal("review must open junk details, not start scanning or cleaning")
				}
			}
		})
	}
}

func TestDashboardCleanProgressBar(t *testing.T) {
	snap := core.CleanSnapshot{TotalFiles: 2000, Files: 1500, TotalBytes: 2048, Bytes: 1024, Failed: 2, Current: "/tmp/cache"}
	stopped := 0
	s := appui.Dashboard{Language: appui.English, Cleaning: true, CleanProgress: &snap, Stop: func() { stopped++ }}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderPage(c, appui.Junk, appui.DashboardActions{}, func() {}) }, 1280, 880)
	for _, label := range []string{"Deleting permanently", "1,500 / 2,000 files", "2 failed", "/tmp/cache", "Stop"} {
		if !tt.HasText(label) {
			t.Fatalf("missing %q: %v", label, tt.Texts())
		}
	}
	if err := tt.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	if stopped != 1 {
		t.Fatal("stop did not dispatch")
	}
	snap.Cancelled = true
	s.Language = appui.Chinese
	tt.Frame()
	if !tt.HasText("正在停止…") || !tt.HasText("1,500 / 2,000 个文件") || !tt.HasText("失败 2") {
		t.Fatalf("cancelled copy: %v", tt.Texts())
	}
	if err := tt.Click("停止"); err != nil {
		t.Fatal(err)
	}
	if stopped != 1 {
		t.Fatal("cancelled bar dispatched another stop")
	}
	s.Cleaning = false
	tt.Frame()
	if tt.HasText("正在停止…") {
		t.Fatal("idle shell still showed the clean bar")
	}
}

func TestDashboardLanguageAndPermissionFacts(t *testing.T) {
	s := appui.Dashboard{Scanned: true, HasJunk: true, Categories: 3, Cleanable: "1.25 GB", Freed: "0 B", MacOS: true}
	opened := 0
	actions := appui.DashboardActions{
		SetLanguage:     func(l appui.Language) { s.Language = l },
		OpenPermissions: func() { opened++ },
	}
	tt := ui.NewTester(func(c *ui.Context) { s.Render(c, actions) }, 1280, 880)
	if !tt.HasText("已在 3 个类别中发现 1.25 GB 可清理内容。") {
		t.Fatal("missing Chinese scan result")
	}
	if err := tt.Click("English"); err != nil {
		t.Fatal(err)
	}
	if !tt.HasText("Found 1.25 GB cleanable items across 3 categories.") || tt.HasText("查看详情并清理") {
		t.Fatal("language switch must translate results without changing their values")
	}
	if err := tt.Click("Limited Access (Configure)"); err != nil {
		t.Fatal(err)
	}
	if opened != 1 {
		t.Fatal("limited macOS access must open permission configuration")
	}
	s.FullDiskAccess = true
	tt.Frame()
	if err := tt.Click("Full Disk Access"); err != nil {
		t.Fatal(err)
	}
	s.MacOS, s.Elevated = false, false
	tt.Frame()
	if !tt.HasText("Standard user") || tt.HasText("Full Disk Access") {
		t.Fatal("Windows permission badge must reflect elevation, not macOS permissions")
	}
	s.Elevated = true
	tt.Frame()
	if err := tt.Click("Administrator"); err != nil {
		t.Fatal(err)
	}
	if opened != 1 {
		t.Fatal("granted or Windows badges must not open macOS permissions")
	}
}

func TestDashboardStatusHeaderRouting(t *testing.T) {
	for _, tc := range []struct {
		language                          appui.Language
		hardware, processes, live, paused string
	}{
		{appui.English, "Hardware", "Processes", "Live", "Paused"},
		{appui.Chinese, "硬件概览", "进程管理", "实时监测", "已暂停"},
	} {
		s := appui.Dashboard{Language: tc.language, ProcessCount: 1234}
		scans := 0
		a := appui.DashboardActions{SetStatusTab: func(tab appui.StatusTab) { s.StatusTab = tab }, ToggleMonitoring: func() { s.MonitoringPaused = !s.MonitoringPaused }, Scan: func() { scans++ }}
		tt := ui.NewTester(func(c *ui.Context) { s.RenderPage(c, appui.Status, a, func() { ui.Text(c, "sample body") }) }, 1280, 880)
		if !tt.HasText("1,234") || tt.HasText("Rescan") || tt.HasText("重新扫描") {
			t.Fatal("status header lost count or acquired scan control")
		}
		if err := tt.Click(tc.processes); err != nil {
			t.Fatal(err)
		}
		if s.StatusTab != appui.StatusProcesses {
			t.Fatal("process tab not dispatched")
		}
		if err := tt.Click(tc.live); err != nil {
			t.Fatal(err)
		}
		if !s.MonitoringPaused || !tt.HasText(tc.paused) || !tt.HasText("sample body") {
			t.Fatal("pause lost sampled content or status")
		}
		if err := tt.Click(tc.paused); err != nil {
			t.Fatal(err)
		}
		if s.MonitoringPaused || !tt.HasText(tc.live) {
			t.Fatal("resume not dispatched")
		}
		if err := tt.Click(tc.hardware); err != nil {
			t.Fatal(err)
		}
		if s.StatusTab != appui.StatusHardware || scans != 0 {
			t.Fatal("tab/pause action dispatched scanning")
		}
	}
}

// 侧栏底部是常驻的设置区（语言、版本都在这里）。自动检查更新原本只有应用菜单
// 能改，Windows 移除菜单后必须有页面入口。
func TestSidebarOffersAutoUpdateToggle(t *testing.T) {
	var events []bool
	s := appui.Dashboard{Language: appui.English, Version: "v0.0.16"}
	a := appui.DashboardActions{SetAutoCheck: func(v bool) { events = append(events, v) }}
	tt := ui.NewTester(func(c *ui.Context) { s.Render(c, a) }, 1280, 800)
	if !tt.HasText("Check for updates automatically") {
		t.Fatalf("settings area has no auto-update toggle: %v", tt.Texts())
	}
	if err := tt.Click("Check for updates automatically"); err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || !events[0] {
		t.Fatalf("toggle events=%v, want one true", events)
	}
	s.AutoCheck = true
	tt.Frame()
	if err := tt.Click("Check for updates automatically"); err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 || events[1] {
		t.Fatalf("toggle events=%v, want a second false", events)
	}
}
