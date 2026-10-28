package app

import (
	"path/filepath"
	"testing"

	"github.com/egoist/mygo"
	"github.com/meimingqi222/quick-cleaner/internal/core"
	view "github.com/meimingqi222/quick-cleaner/internal/ui"
)

// trayRoot gives the root a tray handle. The fake is cleared before
// testRoot's Close, which would otherwise destroy a tray that never existed.
func trayRoot(t *testing.T) *Root {
	t.Helper()
	r, _ := testRoot(t)
	r.tray = &mygo.Tray{}
	t.Cleanup(func() { r.tray = nil })
	return r
}

// MyGo's App.Quit fires OnBeforeQuit, then closes every window, and abandons
// the quit when any OnClose prevents it. Tray Quit, the popover's Quit and
// Cmd+Q all take that path.
func TestAdmittedQuitClosesTheMainWindowInsteadOfHidingIt(t *testing.T) {
	r := trayRoot(t)
	hidden := 0
	hide := func() { hidden++ }

	closing := &mygo.CloseEvent{}
	r.onMainClose(closing, hide)
	if !closing.DefaultPrevented() || hidden != 1 {
		t.Fatal("an ordinary close must hide to the tray")
	}

	quit := &mygo.QuitEvent{}
	r.onBeforeQuit(quit)
	if quit.DefaultPrevented() {
		t.Fatal("an idle quit was refused")
	}
	closing = &mygo.CloseEvent{}
	r.onMainClose(closing, hide)
	if closing.DefaultPrevented() || hidden != 1 {
		t.Fatal("the main window hid during a quit; MyGo abandons the quit")
	}
}

func TestQuitWithPendingWorkIsRefusedAndNotRemembered(t *testing.T) {
	r := trayRoot(t)
	r.confirming = true
	quit := &mygo.QuitEvent{}
	r.onBeforeQuit(quit)
	if !quit.DefaultPrevented() || r.quitting {
		t.Fatal("a quit over pending work must be refused without marking the root as quitting")
	}
}

func TestTrayTopMemoryOpensProcessesSortedByMemory(t *testing.T) {
	r, _ := testRoot(t)
	r.status.Sort = view.StatusSort{Key: view.StatusSortName, Ascending: true}
	r.routeOverview(view.OverviewAction{Kind: view.OverviewTopMemory})
	if r.active != view.Status || r.status.Sort != (view.StatusSort{Key: view.StatusSortMemory}) {
		t.Fatalf("active=%v sort=%+v", r.active, r.status.Sort)
	}
}

// A trash-watch leftover opens the frozen residual review and leaves the
// pending list once — the app must not redetect the same bundle as new.
func TestTrayLeftoverOpensReviewOnce(t *testing.T) {
	r, _ := testRoot(t)
	loc := filepath.Join(t.TempDir(), "Example")
	item := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: loc + "/cache"}, core.ResidualCacheDir)
	app := core.InstalledApp{ID: "com.example.app", Name: "Example", RegistrySubpath: "com.example.app"}
	r.trashLeftovers = []trayLeftover{
		{id: "com.example.app", app: app, result: core.ResidualScanResult{AppName: "Example", Items: []core.ResidualItem{item}}},
	}
	r.routeOverview(view.OverviewAction{Kind: view.OverviewReviewLeftovers, Path: "com.example.app"})
	if !r.apps.ResidualsOpen || r.apps.ResidualAppName != "Example" {
		t.Fatalf("review did not open: open=%v name=%q", r.apps.ResidualsOpen, r.apps.ResidualAppName)
	}
	if len(r.trashLeftovers) != 0 {
		t.Fatal("leftover stayed pending after the review opened")
	}
}

func TestTrayLeftoverSelectAllAndSafeOnly(t *testing.T) {
	r, _ := testRoot(t)
	dir := t.TempDir()
	safeItem := core.CertainResidual(core.ResidualKind{Type: core.ResidualFile, Path: filepath.Join(dir, "pref.plist")}, core.ResidualPreferenceFile)
	confirmItem := core.PossibleResidual(core.ResidualKind{Type: core.ResidualFile, Path: filepath.Join(dir, "daemon.plist")}, core.ResidualLaunchDaemon)
	app := core.InstalledApp{ID: "test.app", Name: "TestApp"}
	r.trashLeftovers = []trayLeftover{
		{
			id:  "test.app",
			app: app,
			result: core.ResidualScanResult{
				AppName: "TestApp",
				Items:   []core.ResidualItem{safeItem, confirmItem},
			},
		},
	}

	// 1. Select All
	r.selectAllLeftovers("test.app")
	if !r.trashLeftovers[0].selected[0] || !r.trashLeftovers[0].selected[1] {
		t.Fatalf("both items should be selected on selectAllLeftovers, got: %v", r.trashLeftovers[0].selected)
	}

	// 2. Select Safe only
	r.selectSafeLeftovers("test.app")
	if !r.trashLeftovers[0].selected[0] || r.trashLeftovers[0].selected[1] {
		t.Fatalf("only safe item should be selected on selectSafeLeftovers, got: %v", r.trashLeftovers[0].selected)
	}
}

func TestSurfaceLeftoverAlertSuppressedWhenMainWindowActiveOrResidualsOpen(t *testing.T) {
	r, _ := testRoot(t)
	r.tray = &mygo.Tray{}
	t.Cleanup(func() { r.tray = nil })

	// Case 1: r.apps.ResidualsOpen is true -> must suppress
	r.apps.ResidualsOpen = true
	r.surfaceLeftoverAlert()
	if r.leftoverAlert != nil {
		t.Fatal("surfaceLeftoverAlert should be suppressed when apps.ResidualsOpen is true")
	}

	// Case 2: main window is active and visible in foreground -> must suppress
	r.apps.ResidualsOpen = false
	r.window = &mygo.Window{}
	r.windowHidden = false
	r.surfaceLeftoverAlert()
	if r.leftoverAlert != nil {
		t.Fatal("surfaceLeftoverAlert should be suppressed when main window is active in foreground")
	}
}

func TestInAppUninstallMarksTrashSeen(t *testing.T) {
	r, _ := testRoot(t)
	loc := "/Applications/Test.app"
	uninstall := "example-uninstaller"
	app := core.InstalledApp{ID: "com.test.app", Name: "Test", InstallLocation: &loc, UninstallString: &uninstall}
	r.installed = []core.InstalledApp{app}
	r.uninstallApp = func(core.InstalledApp) error { return nil }

	// Trigger requestUninstall and confirm it
	r.requestUninstall("com.test.app")
	if !r.confirming {
		t.Fatal("requestUninstall should enter confirming state")
	}
	r.resolveConfirmation(r.confirmation.ID, true)

	if !r.trashSeen["com.test.app"] {
		t.Fatal("confirmUninstall must register app in trashSeen to prevent background watcher double alert")
	}
}
