package ui

import (
	"fmt"
	"reflect"
	"strings"
	"testing"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/meimingqi222/quick-cleaner/internal/core"
)

func appsPageFixture() AppsPage {
	return AppsPage{Language: English, TotalSize: "2 GB", FilteredSize: "2 GB", StaleCount: 1, Sort: core.DefaultAppSortState(),
		Apps: []AppsPageApp{{ID: "alpha", Name: "Alpha", Version: "1.2", Publisher: "Acme", LastUsed: "2026-10-20", InstalledDate: "2026-09-01", Size: "2 GB", CanUninstall: true, HasLocation: true}}, View: []int{0}}
}

func TestAppsPageControlsAndSnapshot(t *testing.T) {
	s := appsPageFixture()
	original := s.Apps[0]
	uninstalls, residuals := 0, 0
	a := AppsPageActions{
		RequestUninstall: func(id string) {
			if id != "alpha" {
				t.Fatal(id)
			}
			uninstalls++
		},
		ScanResiduals: func(id string) {
			if id != "alpha" {
				t.Fatal(id)
			}
			residuals++
		},
		SetPreset:             func(p core.AppFilterPreset) { s.Preset = p; s.View = nil },
		SortBy:                func(col core.AppSortColumn) { s.Sort.Toggle(col) },
		SetSearch:             func(q string) { s.Search = q },
		SetTab:                func(tab AppsPageTab) { s.Tab = tab },
		RenderDevEnvironments: func(c *ui.Context) { ui.Text(c, "parent dev page") },
	}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, a) }, 1280, 800)
	for _, label := range []string{"Applications Manager", "Alpha", "v1.2", "Acme", "Last Used", "Total Size: 2 GB"} {
		if !tt.HasText(label) {
			t.Fatalf("missing %q: %v", label, tt.Texts())
		}
	}
	for _, label := range []string{"Uninstall", "Force Clean", "Publisher"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if uninstalls != 1 || residuals != 1 || s.Sort.Column != core.AppSortPublisher {
		t.Fatalf("callbacks: %d %d %+v", uninstalls, residuals, s.Sort)
	}
	if err := tt.Click("Search apps by name or publisher…"); err != nil {
		t.Fatal(err)
	}
	tt.Type("编辑")
	if s.Search != "编辑" {
		t.Fatalf("search = %q", s.Search)
	}
	tt.Key(0, ui.KeyBackspace)
	if s.Search != "编" {
		t.Fatalf("native backspace = %q", s.Search)
	}
	if err := tt.Click("×"); err != nil {
		t.Fatal(err)
	}
	if s.Search != "" {
		t.Fatalf("clear = %q", s.Search)
	}
	if err := tt.Click("Large Apps (>500MB)"); err != nil {
		t.Fatal(err)
	}
	if s.Preset != core.AppFilterLarge || !tt.HasText("No matching applications found") || tt.HasText("Alpha") {
		t.Fatal("parent filter view not reflected")
	}
	if err := tt.Click("Dev Environments"); err != nil {
		t.Fatal(err)
	}
	if !tt.HasText("parent dev page") || tt.HasText("Total Estimated Size") {
		t.Fatal("developer page not delegated")
	}
	if s.Apps[0] != original {
		t.Fatal("render mutated inventory")
	}
}

func TestAppsPageBusyAndNativeCapabilities(t *testing.T) {
	for _, state := range []string{"idle", "residual", "cleaning", "verifying", "incapable", "portable", "mac"} {
		t.Run(state, func(t *testing.T) {
			s := appsPageFixture()
			s.ResidualScanning = state == "residual"
			s.Cleaning = state == "cleaning"
			s.Uninstalling = state == "verifying"
			s.UninstallPhase = AppsPageVerifying
			s.MacOS = state == "mac"
			s.Apps[0].CanUninstall = state != "incapable" && state != "portable"
			s.Apps[0].Discovered = state == "portable"
			u, r := 0, 0
			a := AppsPageActions{RequestUninstall: func(string) { u++ }, ScanResiduals: func(string) { r++ }}
			tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, a) }, 1280, 800)
			if err := tt.Click("Uninstall"); err != nil {
				t.Fatal(err)
			}
			want := 0
			if state == "idle" || state == "mac" {
				want = 1
			}
			if u != want {
				t.Fatalf("uninstall count=%d want=%d", u, want)
			}
			if s.MacOS {
				if tt.HasText("Force Clean") || !tt.HasText("App Leftovers") {
					t.Fatal("mac platform distinction lost")
				}
				return
			}
			label := "Force Clean"
			if state == "portable" {
				label = "Remove files"
				if !tt.HasText("Unregistered · No official uninstaller") {
					t.Fatal("missing portable evidence label")
				}
			}
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
			want = 1
			if s.busy() {
				want = 0
			}
			if r != want {
				t.Fatalf("residual count=%d want=%d", r, want)
			}
			if state == "verifying" {
				for _, label := range []string{"Verifying the uninstall result", "Analyze associated files", "Remove application", "Verify remaining files"} {
					if !tt.HasText(label) {
						t.Fatalf("missing uninstall stage %q: %v", label, tt.Texts())
					}
				}
			}
		})
	}
}

func TestAppsPageContextMenu(t *testing.T) {
	s := appsPageFixture()
	var calls []string
	a := AppsPageActions{OpenLocation: func(id string) { calls = append(calls, "open:"+id) }, CopyInstallPath: func(id string) { calls = append(calls, "copy:"+id) }, RequestUninstall: func(id string) { calls = append(calls, "uninstall:"+id) }, ScanResiduals: func(id string) { calls = append(calls, "scan:"+id) }}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, a) }, 1280, 800)
	for _, label := range []string{"Open Install Location", "Copy Install Path", "Standard Uninstall", "Force Residual Clean"} {
		if err := tt.RightClick("Alpha"); err != nil {
			t.Fatal(err)
		}
		if err := tt.ChooseMenuItem(label); err != nil {
			t.Fatal(err)
		}
	}
	if !reflect.DeepEqual(calls, []string{"open:alpha", "copy:alpha", "uninstall:alpha", "scan:alpha"}) {
		t.Fatal(calls)
	}
	s.ResidualScanning = true
	tt.Frame()
	if err := tt.Click("···"); err != nil {
		t.Fatal(err)
	}
	_ = tt.ChooseMenuItem("Standard Uninstall")
	if len(calls) != 4 {
		t.Fatal("busy menu dispatched uninstall")
	}
}

func TestAppsPageResidualReviewAndActionBar(t *testing.T) {
	s := appsPageFixture()
	s.ResidualsOpen = true
	s.ResidualAppName = "Alpha"
	s.ResidualSize = "12 MB"
	s.SelectedSize = "12 MB"
	s.Residuals = []AppsPageResidual{{ID: "cache", Path: "/Users/test/Cache/Alpha", Size: "12 MB", Confidence: core.Certain, Source: core.ResidualCacheDir, Kind: core.ResidualDirectory, Selected: true}}
	clean, closed, recommended, all, clear := 0, 0, 0, 0, 0
	a := AppsPageActions{CleanSelectedResiduals: func() { clean++ }, CloseResiduals: func() { closed++ }, SelectRecommended: func() { recommended++ }, SelectAllResiduals: func() { all++ }, ClearResidualSelection: func() { clear++ }, SetResidualSelected: func(id string, on bool) {
		if id != "cache" {
			t.Fatal(id)
		}
		s.Residuals[0].Selected = on
	}}
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, a) }, 1280, 800)
	for _, label := range []string{"Found 1 residual items for \"Alpha\"", "Certain", "Cache directory", "Directory", "/Users/test/Cache/Alpha"} {
		if !tt.HasText(label) {
			t.Fatalf("missing residual %q", label)
		}
	}
	for _, label := range []string{"Recommended", "Select All", "Clear Selection", "Clean Selected (1) · Free 12 MB"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if clean != 1 || recommended != 1 || all != 1 || clear != 1 {
		t.Fatalf("actions=%d %d %d %d", clean, recommended, all, clear)
	}
	if err := tt.Click(s.Residuals[0].Path); err != nil {
		t.Fatal(err)
	}
	if s.Residuals[0].Selected {
		t.Fatal("checkbox did not dispatch snapshot selection")
	}
	if err := tt.Click("Clean Selected (0) · Free 12 MB"); err != nil {
		t.Fatal(err)
	}
	if clean != 1 {
		t.Fatal("empty selection dispatched cleanup")
	}
	s.Residuals[0].Selected = true
	s.Cleaning = true
	tt.Frame()
	if err := tt.Click("Clean Selected (1) · Free 12 MB"); err != nil {
		t.Fatal(err)
	}
	if clean != 1 {
		t.Fatal("busy cleanup dispatched again")
	}
	s.Cleaning = false
	tt.Frame()
	tt.Key(0, ui.KeyEscape)
	if closed != 1 {
		t.Fatalf("dismissal callbacks=%d", closed)
	}
}

func TestAppsPageBilingualOffscreenLayout(t *testing.T) {
	for _, l := range []Language{Chinese, English} {
		s := appsPageFixture()
		s.Language = l
		tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, AppsPageActions{}) }, 1280, 800)
		copy := appsPageStrings(l)
		for _, label := range []string{copy.heading, copy.stats[0], copy.stats[1], copy.stats[2], "Alpha", fmt.Sprintf(copy.size, "2 GB")} {
			r, ok := tt.Find(label)
			if !ok || r.W <= 0 || r.H <= 0 || r.X < 0 || r.Y < 0 || r.X+r.W > 1280 || r.Y+r.H > 800 {
				t.Fatalf("%q outside body: %+v %v", label, r, ok)
			}
		}
		if l == English {
			for _, s := range tt.Texts() {
				for _, r := range s {
					if unicode.Is(unicode.Han, r) {
						t.Fatalf("Chinese leaked into English: %s", s)
					}
				}
			}
		}
		image := ui.Render(func(c *ui.Context) { s.RenderContent(c, AppsPageActions{}) }, 1280, 800, 1)
		if image.Bounds().Dx() != 1280 || image.Bounds().Dy() != 800 {
			t.Fatal(image.Bounds())
		}
		if image.RGBAAt(1, 1).A == 0 {
			t.Fatal("offscreen body not painted")
		}
		if strings.Contains(fmt.Sprintf(copy.residualTitle, 3, "Alpha"), "%!") {
			t.Fatal("invalid bilingual residual format")
		}
	}
}

func TestAppsRowsStayVisibleInsideShell(t *testing.T) {
	s := appsPageFixture()
	s.Language = Chinese
	dash := Dashboard{Language: Chinese, Version: "v0.0.16", Freed: "0 B"}
	tt := ui.NewTester(func(c *ui.Context) {
		dash.RenderPage(c, Apps, DashboardActions{}, func() {
			s.RenderContent(c, AppsPageActions{})
		})
	}, 1280, 880)
	for _, label := range []string{"软件全生命周期管理", "Alpha", "v1.2"} {
		r, ok := tt.Find(label)
		if !ok || r.W <= 0 || r.H <= 0 || r.Y < 0 || r.Y+r.H > 880 {
			t.Fatalf("%q not visible: ok=%v %+v", label, ok, r)
		}
	}
}

func TestAppsPageKeepsRowsWhileMeasuring(t *testing.T) {
	s := appsPageFixture()
	s.Scanning = true
	uninstalls := 0
	tt := ui.NewTester(func(c *ui.Context) {
		s.RenderContent(c, AppsPageActions{RequestUninstall: func(string) { uninstalls++ }})
	}, 1280, 800)
	if !tt.HasText("Alpha") || tt.HasText("Scanning installed applications…") {
		t.Fatalf("measurement hid the rows: %v", tt.Texts())
	}
	if err := tt.Click("Uninstall"); err != nil {
		t.Fatal(err)
	}
	if uninstalls != 0 {
		t.Fatal("uninstall ran while sizes were still being measured")
	}
	s.Apps, s.View = nil, nil
	tt.Frame()
	if !tt.HasText("Scanning installed applications…") || tt.HasText("Alpha") {
		t.Fatal("empty scan lost its loading state")
	}
}

func TestAppsPageVirtualRowsAndStableActions(t *testing.T) {
	s := appsPageFixture()
	s.Apps, s.View = nil, nil
	for i := 0; i < 500; i++ {
		s.Apps = append(s.Apps, AppsPageApp{ID: fmt.Sprint(i), Name: fmt.Sprintf("Application %03d", i), CanUninstall: true})
		s.View = append(s.View, i)
	}
	var target string
	tt := ui.NewTester(func(c *ui.Context) {
		s.RenderContent(c, AppsPageActions{RequestUninstall: func(id string) { target = id }})
	}, 1280, 800)
	if tt.HasText("Application 499") {
		t.Fatal("offscreen row constructed instead of virtualized")
	}
	if err := tt.Click("Uninstall"); err != nil {
		t.Fatal(err)
	}
	if target != "0" {
		t.Fatal(target)
	}
	s.View = []int{499}
	tt.Frame()
	if err := tt.Click("Uninstall"); err != nil {
		t.Fatal(err)
	}
	if target != "499" || !tt.HasText("Application 499") {
		t.Fatalf("stale action after inventory projection: %s", target)
	}
}

func TestAppsPageManualAndOrphanReview(t *testing.T) {
	for _, l := range []Language{Chinese, English} {
		s := appsPageFixture()
		s.Language = l
		s.ResidualsOpen = true
		s.OrphanLeftovers = true
		s.SelectedSize = "0 B"
		s.Notice = core.NewText("需重启后由系统清除", "Requires restart; the system will remove it")
		s.Residuals = []AppsPageResidual{{ID: "extension", Path: "com.example.extension", Kind: core.ResidualSystemExtension, Source: core.ResidualExtensionSource, Manual: true, Selected: true}}
		opened, cleaned := 0, 0
		tt := ui.NewTester(func(c *ui.Context) {
			s.RenderContent(c, AppsPageActions{OpenSystemExtensionSettings: func() { opened++ }, CleanSelectedResiduals: func() { cleaned++ }})
		}, 1280, 800)
		copy := appsPageStrings(l)
		for _, label := range []string{fmt.Sprintf(copy.orphanTitle, 1), copy.extensionTitle, s.Notice.Get(l)} {
			if !tt.HasText(label) {
				t.Fatalf("missing %q", label)
			}
		}
		if err := tt.Click(copy.extensionOpen); err != nil {
			t.Fatal(err)
		}
		if err := tt.Click(fmt.Sprintf(copy.clean, 0, "0 B")); err != nil {
			t.Fatal(err)
		}
		if opened != 1 || cleaned != 0 {
			t.Fatal("manual system extension offered as destructive cleanup")
		}
		manual := AppsPageApp{Name: "Example", ManualUninstall: true}
		if !strings.Contains(manual.UninstallGuidance(l), "Example") {
			t.Fatal("manual guidance lost app identity")
		}
		manual.ManualUninstall = false
		manual.UninstallDeletesData = true
		if manual.UninstallGuidance(l) != copy.dataWarning {
			t.Fatal("official data warning lost")
		}
	}
}

func TestAppsPageOccupancyBannerStates(t *testing.T) {
	long := strings.Repeat("p", 90)
	for _, tc := range []struct {
		lang   Language
		occ    core.ResidualOccupancy
		want   []string
		absent []string
	}{
		{English, core.ResidualOccupancy{}, nil, []string{"Background processes are still running", "Launch-at-login items are still registered"}},
		{English, core.ResidualOccupancy{Processes: []string{long, "beta", "gamma", "delta"}, LaunchdLabels: []string{"com.a", "com.b", "com.c", "com.d"}}, []string{
			"Background processes are still running — database residue will fail to delete",
			"· " + strings.Repeat("p", 78),
			"· beta",
			"· gamma",
			"· Still registered with launchd (may start at login): com.a, com.b, com.c",
			"…and 2 more",
			"Quit the app completely (menu bar and background agents included), then clean again",
		}, []string{"Launch-at-login items are still registered", "· delta", "com.d"}},
		{Chinese, core.ResidualOccupancy{LaunchdLabels: []string{"com.only"}}, []string{
			"该应用仍登记着开机自启项（当前未在运行），残留清理后可能随登录回归",
			"· launchd 仍有登记（可能开机自启）：com.only",
			"退出应用不会移除开机自启项：到「系统设置 → 通用 → 登录项」里把它移除，再重新清理",
		}, []string{"该应用的后台仍在运行", "…以及另外"}},
	} {
		s := appsPageFixture()
		s.Language = tc.lang
		s.ResidualsOpen = true
		s.Occupancy = tc.occ
		s.Residuals = []AppsPageResidual{{ID: "cache", Path: "/tmp/Alpha", Size: "1 B", Confidence: core.Certain, Source: core.ResidualCacheDir, Kind: core.ResidualDirectory}}
		tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, AppsPageActions{}) }, 1280, 800)
		for _, label := range tc.want {
			if !tt.HasText(label) {
				t.Fatalf("%v missing %q", tc.lang, label)
			}
		}
		for _, label := range tc.absent {
			if tt.HasText(label) {
				t.Fatalf("%v unexpectedly showed %q", tc.lang, label)
			}
		}
	}
}

func TestAppsPageShellSizedBody(t *testing.T) {
	for _, l := range []Language{Chinese, English} {
		s := appsPageFixture()
		s.Language = l
		tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, AppsPageActions{}) }, 1090, 720)
		copy := appsPageStrings(l)
		for _, label := range []string{copy.search, copy.uninstall, copy.force, "···", copy.desktop, copy.dev, fmt.Sprintf(copy.size, "2 GB")} {
			r, ok := tt.Find(label)
			if !ok || r.W <= 0 || r.H <= 0 || r.X < 0 || r.Y < 0 || r.X+r.W > 1090 || r.Y+r.H > 720 {
				t.Fatalf("%q outside shell-sized body: %+v", label, r)
			}
		}
	}
}

// Column titles were bordered, centred buttons that did not line up with the
// cells below them.
func TestAppsTableHeadersAlignWithTheirCells(t *testing.T) {
	s := appsPageFixture()
	s.Apps[0].Size = "123.4 MB"
	tt := ui.NewTester(func(c *ui.Context) { s.RenderContent(c, AppsPageActions{}) }, 1440, 900)
	find := func(label string) ui.Rect {
		t.Helper()
		r, ok := tt.Find(label)
		if !ok {
			t.Fatalf("missing %q: %v", label, tt.Texts())
		}
		return r
	}
	publisher, acme := find("Publisher"), find("Acme")
	if d := publisher.X - acme.X; d > 8 || d < -8 {
		t.Fatalf("Publisher title starts %.0fpx from its cells", d)
	}
	size, value := find("Size ▼"), find("123.4 MB")
	if d := (size.X + size.W) - (value.X + value.W); d > 8 || d < -8 {
		t.Fatalf("Size title ends %.0fpx from its cells", d)
	}
}
