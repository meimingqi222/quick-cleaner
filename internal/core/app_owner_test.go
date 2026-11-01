package core

import (
	"errors"
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
	oldForce := forceAppExit
	forceAppExit = func(platform.CacheOccupant) error { t.Error("unexpected force request"); return nil }
	runningOwnerProcesses = live
	if exit != nil {
		requestAppExit = exit
	} else {
		requestAppExit = func(platform.CacheOccupant) error { t.Error("unexpected exit request"); return nil }
	}
	t.Cleanup(func() { runningOwnerProcesses, requestAppExit, forceAppExit = oldLive, oldExit, oldForce })
}

func TestCleanupClosesBackgroundOwnersAfterGracePeriod(t *testing.T) {
	live := []platform.CacheOccupant{{PID: 10, Name: "chrome.exe", Identity: 100}, {PID: 11, Name: "chrome.exe", Identity: 101}}
	var normal, forced []uint32
	stubOwners(t, func([]string) ([]platform.CacheOccupant, error) { return live, nil }, func(o platform.CacheOccupant) error {
		normal = append(normal, o.PID)
		return nil // A background process has no window and ignores WM_CLOSE.
	})
	forceAppExit = func(o platform.CacheOccupant) error {
		forced = append(forced, o.PID)
		if o.Identity != uint64(90+o.PID) {
			t.Fatal("force request lost process identity")
		}
		if len(forced) == 2 {
			live = nil
		}
		return nil
	}
	app := RunningApp{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	if still := CloseRunningAppsForCleanup([]RunningApp{app}, 0, nil); len(still) != 0 {
		t.Fatalf("owners still running: %v", still)
	}
	if !slices.Equal(normal, []uint32{10, 11}) || !slices.Equal(forced, normal) {
		t.Fatalf("normal=%v forced=%v", normal, forced)
	}
}

func TestCancelledCleanupDoesNotCloseOwners(t *testing.T) {
	stubOwners(t, func([]string) ([]platform.CacheOccupant, error) {
		return []platform.CacheOccupant{{PID: 10, Name: "chrome.exe"}}, nil
	}, nil)
	app := RunningApp{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	if still := CloseRunningAppsForCleanup([]RunningApp{app}, 0, func() bool { return true }); len(still) != 1 {
		t.Fatalf("cancelled owner lost: %v", still)
	}
}

func TestCleanupDoesNotForceExitedOrUnobservableOwners(t *testing.T) {
	app := RunningApp{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	t.Run("normal exit", func(t *testing.T) {
		live := true
		stubOwners(t, func([]string) ([]platform.CacheOccupant, error) {
			if live {
				return []platform.CacheOccupant{{PID: 10, Name: "chrome.exe"}}, nil
			}
			return nil, nil
		}, func(platform.CacheOccupant) error { live = false; return nil })
		if still := CloseRunningAppsForCleanup([]RunningApp{app}, 0, nil); len(still) != 0 {
			t.Fatalf("exited owner: %v", still)
		}
	})
	t.Run("unknown", func(t *testing.T) {
		stubOwners(t, func([]string) ([]platform.CacheOccupant, error) { return nil, errors.New("unreadable") }, nil)
		if still := CloseRunningAppsForCleanup([]RunningApp{app}, 0, nil); len(still) != 1 {
			t.Fatalf("unknown treated as exited: %v", still)
		}
	})
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

func TestOwnerEnumerationFailureKeepsCandidatesAndStopsWaiting(t *testing.T) {
	calls := 0
	stubOwners(t, func([]string) ([]platform.CacheOccupant, error) {
		calls++
		return nil, errors.New("unreadable process list")
	}, nil)
	owner := &AppOwner{Name: SameText("Chrome"), Processes: []string{"chrome.exe"}}
	apps := FindRunningApps([]ScanItem{{Path: "/cache", Owner: owner}})
	if len(apps) != 1 || len(apps[0].Paths) != 1 {
		t.Fatalf("unobservable owner treated as absent: %+v", apps)
	}
	// Cancel on the second enumeration so a broken wait cannot delay the test.
	calls = 0
	still := CloseRunningApps(apps, 8*time.Second, func() bool { return calls > 1 })
	if len(still) != 1 || calls != 1 {
		t.Fatalf("unobservable owner waited or disappeared: calls=%d still=%v", calls, still)
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

func TestFindRunningAppsMergesDeclaredBrowsersAndLockedDesktopApps(t *testing.T) {
	oldLocked := lockedCacheOwners
	t.Cleanup(func() { lockedCacheOwners = oldLocked })

	stubOwners(t, func(names []string) ([]platform.CacheOccupant, error) {
		return []platform.CacheOccupant{
			{PID: 100, Name: "msedge.exe", Executable: `C:\Program Files\Microsoft\Edge\Application\msedge.exe`},
			{PID: 200, Name: "Notion.exe", Executable: `C:\Users\alice\AppData\Local\Programs\Notion\Notion.exe`},
			{PID: 201, Name: "Notion.exe", Executable: `C:\Users\alice\AppData\Local\Programs\Notion\Notion.exe`},
			{PID: 999, Name: "Notion.exe", Executable: `D:\OtherVendor\Notion\Notion.exe`},
		}, nil
	}, nil)

	lockedCacheOwners = func(paths []string) ([]platform.LockedTargetOwner, error) {
		notionA := platform.CacheOccupant{PID: 200, Name: "Notion.exe", Executable: `C:\Users\alice\AppData\Local\Programs\Notion\Notion.exe`}
		notionB := platform.CacheOccupant{PID: 201, Name: "Notion.exe", Executable: `C:\Users\alice\AppData\Local\Programs\Notion\Notion.exe`}
		edge := platform.CacheOccupant{PID: 100, Name: "msedge.exe", Executable: `C:\Program Files\Microsoft\Edge\Application\msedge.exe`}
		return []platform.LockedTargetOwner{
			{Target: "/edge/cache", Display: "Microsoft Edge", Owner: edge},
			{Target: "/notion/gpu", Display: "Notion", Owner: notionA},
			{Target: "/notion/gpu", Display: "Notion", Owner: notionB},
			{Target: "/notion/shader", Display: "Notion", Owner: notionA},
		}, nil
	}

	edgeOwner := &AppOwner{Name: SameText("Microsoft Edge"), Processes: []string{"msedge.exe"}}
	items := []ScanItem{
		{Path: "/edge/cache", Size: 100, Disposal: Permanent, Operation: Operation{Kind: OpContents}, Owner: edgeOwner},
		{Path: "/edge/code", Size: 50, Disposal: Permanent, Operation: Operation{Kind: OpContents}, Owner: edgeOwner},
		{Path: "/notion/gpu", Size: 30, Disposal: Permanent, Operation: Operation{Kind: OpContents}},
		{Path: "/notion/shader", Size: 20, Disposal: Permanent, Operation: Operation{Kind: OpContents}},
		{Path: "/unheld/temp", Size: 40, Disposal: Permanent, Operation: Operation{Kind: OpContents}},
	}

	apps := FindRunningApps(items)
	if len(apps) != 2 {
		t.Fatalf("apps = %+v; want 2 merged apps (Edge + Notion)", apps)
	}
	if apps[0].Name.Get(English) != "Microsoft Edge" || apps[0].Bytes != 150 || len(apps[0].Paths) != 2 {
		t.Fatalf("edge merged entry = %+v", apps[0])
	}
	if apps[1].Name.Get(English) != "Notion" || apps[1].Bytes != 50 || len(apps[1].Paths) != 2 || len(apps[1].Live) != 2 {
		t.Fatalf("notion merged entry = %+v", apps[1])
	}

	// Closing Notion must target only its verified executable path, never PID 999.
	var closedPIDs []uint32
	requestAppExit = func(o platform.CacheOccupant) error {
		closedPIDs = append(closedPIDs, o.PID)
		return nil
	}
	_ = CloseRunningApps([]RunningApp{apps[1]}, 0, nil)
	if slices.Contains(closedPIDs, 999) || !slices.Equal(closedPIDs, []uint32{200, 201}) {
		t.Fatalf("executable-scoped close targeted wrong PIDs: %v", closedPIDs)
	}
}
