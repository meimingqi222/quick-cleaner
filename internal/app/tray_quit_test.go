package app

import (
	"testing"

	"github.com/egoist/mygo"
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
