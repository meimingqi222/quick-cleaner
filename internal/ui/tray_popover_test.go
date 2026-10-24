package ui_test

import (
	"fmt"
	"reflect"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func trayPopoverFixture(l appui.Language, volumes int) appui.TrayPopover {
	temp := float32(65)
	s := appui.TrayPopover{Language: l, Snapshot: appui.TrayOverviewSnapshot{
		TraySnapshot: core.TraySnapshot{MemoryUsed: 25 << 30, MemoryTotal: 32 << 30, CPUPct: 42, TempC: &temp},
		Accounts:     []core.LocalAccount{{Name: "user0", Path: "/fixture/user0", IsCurrent: true}},
	}}
	for i := 0; i < volumes; i++ {
		s.Snapshot.Volumes = append(s.Snapshot.Volumes, appui.TrayVolumeInfo{Label: fmt.Sprintf("V%d", i), TotalBytes: 1000, FreeBytes: 500})
	}
	return s
}

func trayPopoverTester(s *appui.TrayPopover, a appui.TrayPopoverActions) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) { s.Render(c, a) }, 360, 560)
}

func TestTrayPopoverPublicActions(t *testing.T) {
	for _, l := range []appui.Language{appui.Chinese, appui.English} {
		s := trayPopoverFixture(l, 2)
		var events []appui.OverviewAction
		dismissed := 0
		a := appui.TrayPopoverActions{Emit: func(a appui.OverviewAction) { events = append(events, a) }, Dismiss: func() { dismissed++ }}
		tt := trayPopoverTester(&s, a)
		labels := []string{"V0", "V1", "释放内存", "查看进程", "智能清理", "软件管理", "文件搜索", "冗余整理", "打开主界面", "退出程序"}
		close := "关闭窗口"
		if l == appui.English {
			labels = []string{"V0", "V1", "Purge RAM", "Processes", "Smart Clean", "Apps", "File Search", "Declutter", "Open QuickCleaner", "Quit QuickCleaner"}
			close = "Close Window"
		}
		want := []appui.OverviewAction{
			{Kind: appui.OverviewAnalyzeDisk, Path: "V0"}, {Kind: appui.OverviewAnalyzeDisk, Path: "V1"},
			{Kind: appui.OverviewReleaseMemory}, {Kind: appui.OverviewOpenView, View: appui.Status},
			{Kind: appui.OverviewScanJunk}, {Kind: appui.OverviewOpenView, View: appui.Apps},
			{Kind: appui.OverviewOpenView, View: appui.Search}, {Kind: appui.OverviewOpenView, View: appui.Declutter},
			{Kind: appui.OverviewOpenMain}, {Kind: appui.OverviewQuit},
		}
		for i, label := range labels {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
			if len(events) != i+1 || events[i] != want[i] {
				t.Fatalf("%q: events = %+v, want %+v", label, events, want[:i+1])
			}
			if dismissed != 0 {
				t.Fatal("content implemented host dismissal policy")
			}
		}
		if err := tt.Click(close); err != nil {
			t.Fatal(err)
		}
		if dismissed != 1 || !reflect.DeepEqual(events, want) {
			t.Fatal("close must only request session dismissal")
		}
		if s.Snapshot.Purged || s.Snapshot.Purging || s.Snapshot.PurgeFailed {
			t.Fatal("content changed the snapshot")
		}
	}
}

func TestTrayPopoverBilingualSnapshotsAndFeedback(t *testing.T) {
	for _, tc := range []struct {
		l                                                           appui.Language
		title, disks, hardware, tools, loading, memory, unavailable string
		purge, purging, purged, failed, free                        string
	}{
		{appui.English, "QuickCleaner Space Lens", "Disk Space Usage", "Hardware Status", "Quick Tools", "Loading…", "RAM 78%", "Temperature unavailable", "Purge RAM", "Releasing…", "Purged", "Failed, retry", "Free 500 B (50%)"},
		{appui.Chinese, "QuickCleaner 空间透镜", "磁盘空间占用", "硬件核心状态", "快捷功能", "正在读取…", "内存 78%", "温度不可用", "释放内存", "正在释放…", "已释放", "释放失败，重试", "剩余 500 B (50%)"},
	} {
		s := trayPopoverFixture(tc.l, 0)
		s.Snapshot.TempC = nil
		var events []appui.OverviewAction
		tt := trayPopoverTester(&s, appui.TrayPopoverActions{Emit: func(a appui.OverviewAction) { events = append(events, a) }})
		for _, label := range []string{tc.title, tc.disks, tc.hardware, tc.tools, tc.loading, tc.memory, tc.unavailable, tc.purge, "CPU 42%", "25.00 GB / 32.00 GB"} {
			if !tt.HasText(label) {
				t.Errorf("missing %q: %v", label, tt.Texts())
			}
		}
		if len(events) != 0 {
			t.Fatal("render emitted an action")
		}
		// A fresh controller snapshot updates counters without remounting content.
		s.Snapshot.Volumes = []appui.TrayVolumeInfo{{Label: "V0", TotalBytes: 1000, FreeBytes: 500}}
		s.Snapshot.CPUPct = 83
		temp := float32(86)
		s.Snapshot.TempC = &temp
		tt.Frame()
		for _, label := range []string{tc.free, "CPU 83%", "86°C"} {
			if !tt.HasText(label) {
				t.Errorf("missing %q", label)
			}
		}
		if tt.HasText(tc.loading) || tt.HasText(tc.unavailable) || tt.HasText("CPU 42%") {
			t.Fatal("stale sample rendered")
		}
		for _, feedback := range []struct {
			purging, purged, failed bool
			label                   string
		}{
			{true, false, false, tc.purging}, {false, true, false, tc.purged}, {false, false, true, tc.failed},
			{true, true, true, tc.purging}, {false, true, true, tc.failed}, {false, false, false, tc.purge},
		} {
			s.Snapshot.Purging, s.Snapshot.Purged, s.Snapshot.PurgeFailed = feedback.purging, feedback.purged, feedback.failed
			tt.Frame()
			if !tt.HasText(feedback.label) {
				t.Fatal(tt.Texts())
			}
			before := len(events)
			if err := tt.Click(feedback.label); err != nil {
				t.Fatal(err)
			}
			if len(events) != before+1 || events[before].Kind != appui.OverviewReleaseMemory {
				t.Fatal("release policy must stay in controller, including in-flight/success states")
			}
		}
		if tt.HasText("user0") || tt.HasText("Local Accounts") || tt.HasText("本地账户与目录") {
			t.Fatal("current Rust content does not render accounts; do not resurrect an unused legacy section")
		}
		if got := tt.Image().Bounds(); got.Dx() != 360 || got.Dy() != 560 {
			t.Fatal(got)
		}
	}
}

func TestTrayPopoverAdaptiveLayoutAndScrolling(t *testing.T) {
	for _, l := range []appui.Language{appui.Chinese, appui.English} {
		for _, count := range []int{1, 2, 3, 4, 5, 6, 10, 30} {
			t.Run(fmt.Sprintf("%d/%d", l, count), func(t *testing.T) {
				s := trayPopoverFixture(l, count)
				var events []appui.OverviewAction
				tt := trayPopoverTester(&s, appui.TrayPopoverActions{Emit: func(a appui.OverviewAction) { events = append(events, a) }})
				hardware, quit := "Hardware Status", "Quit QuickCleaner"
				if l == appui.Chinese {
					hardware, quit = "硬件核心状态", "退出程序"
				}
				hw := trayPopoverFind(t, tt, hardware)
				footer := trayPopoverFind(t, tt, quit)
				if hw.Y < 0 || footer.Y+footer.H > 560 || hw.Y >= footer.Y {
					t.Fatalf("hardware/footer outside viewport: %v / %v", hw, footer)
				}
				first := trayPopoverFind(t, tt, "V0")
				if count <= 6 {
					last := trayPopoverFind(t, tt, fmt.Sprintf("V%d", count-1))
					if last.Y+last.H+12 > hw.Y {
						t.Fatalf("%d volumes must fit with margin above hardware: %v / %v", count, last, hw)
					}
				}
				if count >= 4 {
					countLabel := fmt.Sprintf("(%d vols)", count)
					if l == appui.Chinese {
						countLabel = fmt.Sprintf("(%d 卷)", count)
					}
					if !tt.HasText(countLabel) {
						t.Fatal("missing localized volume count", tt.Texts())
					}
					second := trayPopoverFind(t, tt, "V1")
					if first.Y != second.Y || second.X <= first.X {
						t.Fatal("4+ volumes must form two columns")
					}
					if count%2 == 1 {
						last := trayPopoverFind(t, tt, fmt.Sprintf("V%d", count-1))
						if last.X != first.X {
							t.Fatal("odd final volume must stay in first column")
						}
					}
				}
				if count >= 10 {
					tt.Scroll(150, first.Y+first.H/2, 0, 180)
					if after := trayPopoverFind(t, tt, "V0"); after.Y >= first.Y {
						t.Fatalf("disk list did not scroll: %v -> %v", first, after)
					}
					if after := trayPopoverFind(t, tt, hardware); after != hw {
						t.Fatal("disk scrolling moved pinned hardware")
					}
					if after := trayPopoverFind(t, tt, quit); after != footer {
						t.Fatal("disk scrolling moved pinned actions")
					}
					tt.Scroll(150, hw.Y-30, 0, 10000)
					label := fmt.Sprintf("V%d", count-1)
					last := trayPopoverFind(t, tt, label)
					if last.Y < 40 || last.Y+last.H >= hw.Y {
						t.Fatal("last disk must be reachable inside its scroll viewport", last, hw)
					}
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
					if len(events) != 1 || events[0] != (appui.OverviewAction{Kind: appui.OverviewAnalyzeDisk, Path: label}) {
						t.Fatal("scrolled volume action lost its identity", events)
					}
					s.Snapshot.CPUPct++
					tt.Frame()
					if after := trayPopoverFind(t, tt, label); after != last {
						t.Fatal("sampling update reset disk scroll position", last, after)
					}
				}
				if count == 4 || count == 6 {
					label := fmt.Sprintf("V%d", count-1)
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
					if len(events) != 1 || events[0] != (appui.OverviewAction{Kind: appui.OverviewAnalyzeDisk, Path: label}) {
						t.Fatal("compact volume action lost its identity", events)
					}
				}
			})
		}
	}
}

func trayPopoverFind(t *testing.T, tt *ui.Tester, label string) ui.Rect {
	t.Helper()
	r, ok := tt.Find(label)
	if !ok {
		t.Fatalf("missing %q: %v", label, tt.Texts())
	}
	return r
}

func TestTrayPopoverZeroAndInvalidCapacityAndUnboundActions(t *testing.T) {
	s := appui.TrayPopover{Language: appui.English, Snapshot: appui.TrayOverviewSnapshot{Volumes: []appui.TrayVolumeInfo{{Label: "Empty"}, {Label: "Changed", TotalBytes: 100, FreeBytes: 200}}}}
	tt := trayPopoverTester(&s, appui.TrayPopoverActions{})
	for _, label := range []string{"RAM 0%", "CPU 0%", "0 B / 0 B", "Free 0 B (0%)", "0 B / 100 B", "Free 200 B (0%)"} {
		if !tt.HasText(label) {
			t.Errorf("missing %q: %v", label, tt.Texts())
		}
	}
	for _, label := range []string{"Empty", "Changed", "Purge RAM", "Processes", "Smart Clean", "Open QuickCleaner", "Quit QuickCleaner", "Close Window"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
}
