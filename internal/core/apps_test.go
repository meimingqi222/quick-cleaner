package core

import (
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"testing"
)

func TestAppsCommandParsing(t *testing.T) {
	real := `C:\Program Files (x86)\Vendor\uninstall.exe`
	for _, tc := range []struct {
		cmd, exe string
		args     []string
	}{
		{real + " /S", real, []string{"/S"}},
		{`"` + real + `" /S /NORESTART`, real, []string{"/S", "/NORESTART"}},
		{`"` + real + ` /SLIENT"`, real, []string{"/SLIENT"}},
		{`"C:\Gone\unins 2.exe" /S`, `C:\Gone\unins 2.exe`, []string{"/S"}},
		{"winget uninstall --id Foo", "winget", []string{"uninstall", "--id", "Foo"}},
		{`C:\Gone\unins.exe /S`, `C:\Gone\unins.exe`, []string{"/S"}},
		{`C:\Foo.exe bar --flag`, `C:\Foo.exe`, []string{"bar", "--flag"}},
	} {
		exe, args := SplitCommandWith(tc.cmd, func(p string) bool { return p == real || p == `C:\Foo.exe` || p == `C:\Foo.exe bar` })
		if exe != tc.exe || !reflect.DeepEqual(args, tc.args) {
			t.Errorf("%q => %q %q", tc.cmd, exe, args)
		}
	}
	if exe, args := SplitCommandWith("  ", func(string) bool { return true }); exe != "" || len(args) != 0 {
		t.Fatal(exe, args)
	}
	if got := ParseCmdLine(` /D="C:\literal\path with spaces\" ` + "\t" + `"" /S`); !reflect.DeepEqual(got, []string{`/D=C:\literal\path with spaces\`, "/S"}) {
		t.Fatal(got)
	}
	root := t.TempDir()
	prefix := filepath.Join(root, "Program Files")
	exe := prefix + " (x86) uninstall.exe"
	if err := os.Mkdir(prefix, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(exe, nil, 0600); err != nil {
		t.Fatal(err)
	}
	if got, args := SplitCommand(exe + " /S"); got != exe || !reflect.DeepEqual(args, []string{"/S"}) {
		t.Fatalf("directory prefix won: %q %v", got, args)
	}
}

func TestAppsTimestampSortsAndFilters(t *testing.T) {
	apps := []InstalledApp{{Name: "old", LastUsedRaw: 100, InstallDateRaw: 100}, {Name: "never"}, {Name: "recent", LastUsedRaw: 500, InstallDateRaw: 500}}
	for _, column := range []AppSortColumn{AppSortLastUsed, AppSortInstallDate} {
		for _, dir := range []SortDirection{Ascending, Descending} {
			want := []int{0, 2, 1}
			if dir == Descending {
				want = []int{2, 0, 1}
			}
			if got := FilterAndSortApps(apps, AppFilterAll, "", AppSortState{column, dir}); !reflect.DeepEqual(got, want) {
				t.Fatal(column, dir, got)
			}
		}
	}
	now := RarelyUsedSecs + 100
	for _, tc := range []struct {
		raw  uint64
		want bool
	}{{0, true}, {100, false}, {99, true}, {now + 1, false}} {
		if got := IsRarelyUsed(InstalledApp{LastUsedRaw: tc.raw}, now); got != tc.want {
			t.Fatal(tc, got)
		}
	}
	if !AppFilterLarge.Matches(InstalledApp{EstimatedSize: 500 * 1024 * 1024}, now) || AppFilterLarge.Matches(InstalledApp{EstimatedSize: 500*1024*1024 - 1}, now) {
		t.Fatal("large boundary")
	}
	apps = []InstalledApp{{ID: "id-match", Name: "z", Publisher: "Acme", EstimatedSize: 3}, {Name: "a", EstimatedSize: 3}, {Name: "b", EstimatedSize: 5}}
	if got := FilterAndSortApps(apps, AppFilterAll, " ACME ", DefaultAppSortState()); !reflect.DeepEqual(got, []int{0}) {
		t.Fatal(got)
	}
	if got := FilterAndSortApps(apps, AppFilterAll, "ID-MATCH", DefaultAppSortState()); !reflect.DeepEqual(got, []int{0}) {
		t.Fatal(got)
	}
	if got := FilterAndSortApps(apps, AppFilterAll, "", DefaultAppSortState()); !reflect.DeepEqual(got, []int{2, 1, 0}) {
		t.Fatal(got)
	}
	state := DefaultAppSortState()
	state.Toggle(AppSortName)
	if state.Direction != Ascending || state.Indicator(AppSortName) != " ▲" {
		t.Fatal(state)
	}
	state.Toggle(AppSortName)
	if state.Direction != Descending || state.Indicator(AppSortSize) != "" {
		t.Fatal(state)
	}
	unicodeApps := []InstalledApp{{Name: "ΟΣ"}, {Name: "Ος"}, {Name: "İx"}, {Name: "iz"}}
	if got := FilterAndSortApps(unicodeApps, AppFilterAll, "i\u0307", DefaultAppSortState()); !reflect.DeepEqual(got, []int{2}) {
		t.Fatalf("Unicode lowercase expansion lost during search: %v", got)
	}
	if got := FilterAndSortApps(unicodeApps, AppFilterAll, "ος", AppSortState{AppSortName, Ascending}); !reflect.DeepEqual(got, []int{1, 0}) {
		t.Fatalf("search uses contextual lowercase, sorting uses character lowercase: %v", got)
	}
	if got := FilterAndSortApps(unicodeApps, AppFilterAll, "", AppSortState{AppSortName, Ascending}); !reflect.DeepEqual(got, []int{3, 2, 1, 0}) {
		t.Fatalf("Rust Unicode sort order changed: %v", got)
	}
}

func TestAppsModelsAndParsing(t *testing.T) {
	for r := AppHKLM; r <= AppUnregistered; r++ {
		if r.Label() == r.LabelLang(Chinese) || r.Label() == r.LabelLang(English) {
			t.Fatal("neutral label", r)
		}
	}
	for _, tc := range []struct{ raw, want string }{{`"C:\Program Files\app.exe",0`, `C:\Program Files\app.exe`}, {`C:\app.exe,-1`, `C:\app.exe`}, {`C:\comma,name.ico`, `C:\comma,name.ico`}, {"", ""}} {
		got, ok := DisplayIconPath(tc.raw)
		if got != tc.want || ok != (tc.want != "") {
			t.Fatal(tc, got, ok)
		}
	}
	location, icon := "fallback", `C:\icon.exe,0`
	app := InstalledApp{InstallLocation: &location, DisplayIcon: &icon}
	if p, _ := app.IconCacheKey(); p != `C:\icon.exe` {
		t.Fatal(p)
	}
	app.DisplayIcon = nil
	if p, _ := app.IconCacheKey(); p != location {
		t.Fatal(p)
	}
	command := ""
	app.UninstallString = &command
	if !app.CanUninstall() {
		t.Fatal("present command")
	}
	app.Discovery = &AppDiscovery{}
	if runtime.GOOS != "darwin" && app.CanUninstall() {
		t.Fatal("discovery has no official authority")
	}
	if AppFilterOrphan.Matches(app, 0) {
		t.Fatal("discovery is not missing")
	}
	app.UninstallerMissing = true
	if !AppFilterOrphan.Matches(app, 0) {
		t.Fatal("missing")
	}
	app.IsSystemComponent = true
	if app.CanUninstall() {
		t.Fatal("system component")
	}
	for _, s := range []string{" app ", "Microsoft", "WINDOWS", "program files (x86)", "Tencent", "ab"} {
		if IsSafeAppToken(s) {
			t.Fatal(s)
		}
	}
	if !IsSafeAppToken("SpecificProduct") {
		t.Fatal("specific token")
	}
	for _, tc := range []struct {
		raw, want string
		n         uint64
	}{{"20260102", "2026-01-02", 20260102}, {" 2026-01-02 ", "2026-01-02", 20260102}, {"short", "short", 0}, {"999999999999999999999999", "999999999999999999999999", 0}} {
		s, n := ParseInstallDate(&tc.raw)
		if s == nil || *s != tc.want || n != tc.n {
			t.Fatal(tc, s, n)
		}
	}
	if s, n := ParseInstallDate(nil); s != nil || n != 0 {
		t.Fatal(s, n)
	}
}

func TestAppsResidualDedupAndManualPropagation(t *testing.T) {
	parent := PossibleResidual(ResidualKind{Type: ResidualDirectory, Path: `C:\Vendor`, Bytes: 100}, ResidualLikelyAppDataDir)
	child := CertainResidual(ResidualKind{Type: ResidualFile, Path: `c:/vendor/sub/cache`, Bytes: 10}, ResidualAppDataDir)
	child.RequiresConfirmation = true
	sibling := CertainResidual(ResidualKind{Type: ResidualFile, Path: `C:\VendorExtra\cache`}, ResidualOther)
	for _, items := range [][]ResidualItem{{parent, child, sibling}, {child, sibling, parent}} {
		got := DedupeResiduals(items)
		if len(got) != 2 {
			t.Fatal(got)
		}
		for _, i := range got {
			if i.Kind.Type == ResidualDirectory && (i.Confidence != Possible || !i.RequiresConfirmation) {
				t.Fatal(i)
			}
		}
		if len((ResidualScanResult{Items: got}).DefaultSelection()) != 1 {
			t.Fatal(got)
		}
	}
	duplicate := parent
	duplicate.Kind.Bytes = 40
	duplicate.Confidence = Certain
	duplicate.RequiresConfirmation = true
	for _, items := range [][]ResidualItem{{parent, duplicate}, {duplicate, parent}} {
		got := DedupeResiduals(items)
		if len(got) != 1 || got[0].Size() != 40 || !got[0].RequiresConfirmation || got[0].Confidence != Certain {
			t.Fatal(got)
		}
	}
	key := CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: AppHKCU, Path: `Software\Vendor`}, ResidualConfigRegKey)
	value := CertainResidual(ResidualKind{Type: ResidualRegistryValue, Root: AppHKCU, Path: `software/vendor/sub`, Name: "Install"}, ResidualConfigRegKey)
	value.RequiresConfirmation = true
	otherRoot := key
	otherRoot.Kind.Root = AppHKLM
	neighbor := key
	neighbor.Kind.Path = `Software\Vendor2`
	for _, items := range [][]ResidualItem{{key, value, otherRoot, neighbor}, {neighbor, otherRoot, value, key}} {
		got := DedupeResiduals(items)
		if len(got) != 3 {
			t.Fatal(got)
		}
		for _, i := range got {
			if i.Kind.Root == AppHKCU && i.Kind.Path == key.Kind.Path && !i.RequiresConfirmation {
				t.Fatal("lost manual confirmation")
			}
			if i.Kind.Root == AppHKLM && i.RequiresConfirmation {
				t.Fatal("cross root propagation")
			}
		}
	}
}

func TestAppsResidualIdentityPlansAndOwners(t *testing.T) {
	path := filepath.Join(t.TempDir(), "data")
	if err := os.WriteFile(path, []byte("data"), 0600); err != nil {
		t.Fatal(err)
	}
	file := CertainResidual(ResidualKind{Type: ResidualFile, Path: path, Bytes: 4}, ResidualInstallDir).ForBundle("com.vendor.app")
	if file.Identity == nil || !file.Identity.Recheck(path) {
		t.Fatal("missing snapshot")
	}
	if id, ok := file.Owner(); !ok || id != "com.vendor.app" {
		t.Fatal(id, ok)
	}
	if _, ok := file.ForBundle("").Owner(); ok {
		t.Fatal("empty owner must fail closed")
	}
	if _, ok := (ResidualItem{}).Owner(); ok {
		t.Fatal("unknown owner")
	}
	kinds := []ResidualKind{file.Kind, {Type: ResidualDirectory, Path: filepath.Dir(path)}, {Type: ResidualRegistryKey, Root: AppHKCU, Path: `Software\Vendor`}, {Type: ResidualRegistryValue, Root: AppHKLM32, Path: `Software\Vendor`, Name: "Install"}, {Type: ResidualScheduledTask, Path: `\Vendor\Update`}, {Type: ResidualSystemExtension, TeamID: "TEAM", BundleID: "com.vendor.app"}}
	want := []Operation{{Kind: OpFile}, {Kind: OpTree}, {Kind: OpNative, Native: RegistryKey, Identifier: `HKCU\Software\Vendor`}, {Kind: OpNative, Native: RegistryValue, Identifier: `HKLM32\Software\Vendor → Install`}, {Kind: OpNative, Native: ScheduledTask, Identifier: `\Vendor\Update`}, {Kind: OpNative, Native: SystemExtension, Identifier: "TEAM/com.vendor.app"}}
	items := []ResidualItem{}
	for n, k := range kinds {
		if k.Operation() != want[n] {
			t.Fatal(n, k.Operation())
		}
		items = append(items, CertainResidual(k, ResidualOther))
	}
	result := ResidualScanResult{Items: items}
	plan := result.CleanupPlan()
	if plan == nil || len(plan.Targets()) != len(items) {
		t.Fatal(plan)
	}
	for n, target := range plan.Targets() {
		if target.Operation != want[n] || target.Disposal != Permanent {
			t.Fatal(target)
		}
		if n >= 2 && target.Path != want[n].Identifier {
			t.Fatal(target)
		}
		if _, ok := plan.Scope(n); !ok {
			t.Fatal(n)
		}
	}
	if (ResidualScanResult{}).CleanupPlan() != nil {
		t.Fatal("empty plan")
	}
	if result.CertainCount() != 6 || len(result.DefaultSelection()) != 6 {
		t.Fatal(result)
	}
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("replacement"), 0600); err != nil {
		t.Fatal(err)
	}
	if file.Identity.Recheck(path) {
		t.Fatal("snapshot was not frozen")
	}
}

func TestAppsResidualFollowUpAndCompletion(t *testing.T) {
	items := []ResidualItem{CertainResidual(ResidualKind{Type: ResidualDirectory, Path: "install"}, ResidualInstallDir), CertainResidual(ResidualKind{Type: ResidualRegistryKey, Path: "entry"}, ResidualUninstallEntry), PossibleResidual(ResidualKind{Type: ResidualDirectory, Path: "cache"}, ResidualAppDataDir)}
	calls := 0
	follow := FollowUpResidualClean(items, map[int]struct{}{0: {}, 1: {}}, func(ResidualItem) bool { calls++; return false })
	if calls != 2 || len(follow.RetryItems) != 0 || len(follow.RetrySelected) != 0 || len(follow.LeftoverForApp) != 1 || !AppGoneAfterResidualClean(items, follow.LeftoverForApp) {
		t.Fatal(follow, calls)
	}
	follow = FollowUpResidualClean(items, map[int]struct{}{1: {}}, func(ResidualItem) bool { return true })
	if len(follow.RetryItems) != 1 || follow.RetryItems[0].Source != ResidualUninstallEntry || len(follow.LeftoverForApp) != 3 {
		t.Fatal(follow)
	}
	if _, ok := follow.RetrySelected[0]; !ok {
		t.Fatal("retry indices must be rebased")
	}
	follow = FollowUpResidualClean(items, map[int]struct{}{2: {}}, func(ResidualItem) bool { return false })
	if AppGoneAfterResidualClean(items, follow.LeftoverForApp) || AppGoneAfterResidualClean(items[2:], nil) {
		t.Fatal("cache cleanup is not uninstall completion")
	}
	root := t.TempDir()
	a, b := filepath.Join(root, "app.exe"), filepath.Join(root, "companion.dll")
	for _, p := range []string{a, b} {
		if err := os.WriteFile(p, nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	d := AppDiscovery{ProgramPaths: []string{a, b}}
	if DiscoveredProgramFilesAbsent(d) {
		t.Fatal("present artifacts")
	}
	if err := os.Remove(a); err != nil {
		t.Fatal(err)
	}
	if DiscoveredProgramFilesAbsent(d) {
		t.Fatal("companion remains")
	}
	if err := os.Remove(b); err != nil {
		t.Fatal(err)
	}
	if !DiscoveredProgramFilesAbsent(d) {
		t.Fatal("all absent")
	}
	if DiscoveredProgramFilesAbsent(AppDiscovery{}) || DiscoveredProgramFilesAbsent(AppDiscovery{ProgramPaths: []string{root}}) {
		t.Fatal("absence requires evidence")
	}
}
