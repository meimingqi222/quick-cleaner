package core

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func stubOwners(t *testing.T, live func([]string) ([]platform.CacheOccupant, error), exit func(platform.CacheOccupant) error) {
	t.Helper()
	oldLive, oldExit := runningOwnerProcesses, requestAppExit
	runningOwnerProcesses = live
	if exit != nil {
		requestAppExit = exit
	} else {
		requestAppExit = func(platform.CacheOccupant) error { t.Error("unexpected exit request"); return nil }
	}
	t.Cleanup(func() { runningOwnerProcesses, requestAppExit = oldLive, oldExit })
}

func TestBundledBrowserOwnersAreDeclared(t *testing.T) {
	s, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	for _, row := range []Layout{{Zh: "Chrome", En: "Chrome"}, {Zh: "Edge", En: "Edge"}, {Zh: "Brave", En: "Brave"}} {
		if owner := browserOwner(s, row); owner == nil || len(owner.Processes) != 1 {
			t.Errorf("%s owner = %+v", row.En, owner)
		}
	}
}

// Owner lists may only describe a declared browser row with bare image names.
func TestOwnerProcessListsMustMatchBrowserRows(t *testing.T) {
	base := func() RuleDefinition {
		return RuleDefinition{ID: "browsers", Catalogs: map[string][]Layout{"windows_user_data": {{Path: "Google/Chrome/User Data", Zh: "Chrome", En: "Chrome"}}}, Lists: map[string][]string{"owner_processes_chrome": {"chrome.exe"}}}
	}
	ok := base()
	if err := validateOwnerProcesses(&ok); err != nil {
		t.Fatalf("valid declaration rejected: %v", err)
	}
	for name, change := range map[string]func(*RuleDefinition){
		"unknown row":   func(r *RuleDefinition) { r.Lists["owner_processes_nova"] = []string{"nova.exe"} },
		"path":          func(r *RuleDefinition) { r.Lists["owner_processes_chrome"] = []string{`C:\x\chrome.exe`} },
		"not exe":       func(r *RuleDefinition) { r.Lists["owner_processes_chrome"] = []string{"chrome"} },
		"empty":         func(r *RuleDefinition) { r.Lists["owner_processes_chrome"] = nil },
		"other rule id": func(r *RuleDefinition) { r.ID = "cache" },
	} {
		r := base()
		change(&r)
		if validateOwnerProcesses(&r) == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

func TestFindRunningAppsGroupsByOwnerAndSkipsIdleApps(t *testing.T) {
	stubOwners(t, func(names []string) ([]platform.CacheOccupant, error) {
		if !slices.Contains(names, "chrome.exe") || !slices.Contains(names, "msedge.exe") {
			t.Errorf("owner names not requested: %v", names)
		}
		return []platform.CacheOccupant{{PID: 10, Name: "CHROME.EXE"}, {PID: 11, Name: "chrome.exe"}}, nil
	}, nil)
	chrome := &AppOwner{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	edge := &AppOwner{Name: SameText("Edge"), Processes: []string{"msedge.exe"}}
	apps := FindRunningApps([]ScanItem{
		{Path: "/c/a", Size: 3, Owner: chrome}, {Path: "/c/b", Size: 4, Owner: chrome},
		{Path: "/e/a", Size: 5, Owner: edge}, {Path: "/plain", Size: 6},
	})
	if len(apps) != 1 || apps[0].Name.Get(English) != "Chrome" || len(apps[0].Live) != 2 || apps[0].Bytes != 7 || !slices.Equal(apps[0].Paths, []string{"/c/a", "/c/b"}) {
		t.Fatalf("running apps = %+v", apps)
	}
}

func TestRunningOwnerBusyBadgesOnlyRunningOwners(t *testing.T) {
	stubOwners(t, func([]string) ([]platform.CacheOccupant, error) {
		return []platform.CacheOccupant{{PID: 10, Name: "chrome.exe"}}, nil
	}, nil)
	busy := RunningOwnerBusy([]ScanTarget{
		{Path: "/c", Owner: &AppOwner{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}},
		{Path: "/e", Owner: &AppOwner{Name: SameText("Edge"), Processes: []string{"msedge.exe"}}},
		{Path: "/plain"},
	})
	if len(busy) != 1 || busy["/c"].App != "Chrome" {
		t.Fatalf("badges = %+v", busy)
	}
}

// Closing is a normal quit request only; an app that stays is reported, and
// its locked files are left to the per-file cleaner.
func TestCloseRunningAppsAsksNormallyAndReportsStragglers(t *testing.T) {
	exits := 0
	quit := false
	stubOwners(t, func([]string) ([]platform.CacheOccupant, error) {
		if quit {
			return nil, nil
		}
		return []platform.CacheOccupant{{PID: 10, Name: "chrome.exe"}, {PID: 11, Name: "chrome.exe"}}, nil
	}, func(platform.CacheOccupant) error { exits++; return nil })
	app := RunningApp{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	still := CloseRunningApps([]RunningApp{app}, 50*time.Millisecond, nil)
	if exits != 2 || len(still) != 1 || still[0].Get(English) != "Chrome" {
		t.Fatalf("exits=%d still=%v", exits, still)
	}
	quit = true
	if still := CloseRunningApps([]RunningApp{app}, time.Second, nil); len(still) != 0 {
		t.Fatalf("quit app reported as running: %v", still)
	}
}

func TestWindowsBrowserTargetsCarryTheirOwner(t *testing.T) {
	local := t.TempDir()
	for _, leaf := range []string{"Cache", "Code Cache", "GPUCache"} {
		path := filepath.Join(local, "Google", "Chrome", "User Data", "Default", leaf, "data")
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	s, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	found := 0
	for _, target := range FilesystemScanTargets(s, map[string]string{"home": t.TempDir(), "local": local}, "windows") {
		if target.Category != BrowserCache {
			continue
		}
		found++
		if target.Owner == nil || !slices.Contains(target.Owner.Processes, "chrome.exe") {
			t.Fatalf("browser target without owner: %s %+v", target.Path, target.Owner)
		}
	}
	if found == 0 {
		t.Fatal("fixture produced no browser targets")
	}
}
