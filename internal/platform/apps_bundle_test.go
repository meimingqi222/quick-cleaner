package platform

import (
	"context"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

func TestMacBundleSortUsesContextualFinalSigma(t *testing.T) {
	// Scalar lowercase yields οσ > οςα; contextual lowercase yields ος < οςα.
	if strings.ToLower("ΟΣ") <= strings.ToLower("Οςα") {
		t.Fatal("fixture must distinguish scalar from contextual lowercase")
	}
	apps := []AppRecord{{Name: "Οςα"}, {Name: "ΟΣ"}}
	macSortAppRecords(apps)
	if apps[0].Name != "ΟΣ" || apps[1].Name != "Οςα" {
		t.Fatalf("contextual lowercase order: %+v", apps)
	}
}

func macFixtureBundle(t *testing.T, root, name string) string {
	t.Helper()
	path := filepath.Join(root, name)
	if err := os.MkdirAll(filepath.Join(path, "Contents"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(path, "Contents", "Info.plist"), []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestMacBundleRootsAndDates(t *testing.T) {
	home := t.TempDir()
	roots := macAppRoots(home)
	if len(roots) != 6 || roots[1].path != "/System/Applications" || !roots[1].system || roots[3].path != "/Library/Input Methods" || roots[5].path != filepath.Join(home, "Library", "Input Methods") {
		t.Fatal(roots)
	}
	if len(macAppRoots("")) != 4 {
		t.Fatal("missing home must not scan relative paths")
	}
	for _, raw := range []string{"2026-08-14 05:59:44 +0000", "2026-08-14T05:59:44Z"} {
		d := macMetadataDate(raw)
		if d.date == nil || *d.date != "2026-08-14" || d.raw == 0 {
			t.Fatal(d)
		}
	}
	for _, raw := range []string{"(null)", "", "1970-01-01T00:00:00Z", "invalid"} {
		if macMetadataDate(raw).raw != 0 {
			t.Fatal(raw)
		}
	}
	if macSystemDate(time.Unix(0, 0)).date != nil {
		t.Fatal("epoch is not a date")
	}
}

func TestMacBundleMetadataCommands(t *testing.T) {
	bundle := macFixtureBundle(t, t.TempDir(), "Spaces App.app")
	plist, err := filepath.EvalSymlinks(filepath.Join(bundle, "Contents", "Info.plist"))
	if err != nil {
		t.Fatal(err)
	}
	for _, defaultsOK := range []bool{true, false} {
		t.Run(map[bool]string{true: "defaults", false: "plutil"}[defaultsOK], func(t *testing.T) {
			var calls []string
			runner := func(ctx context.Context, timeout time.Duration, program string, args ...string) (ProcessResult, bool) {
				if timeout != 2*time.Second || ctx.Err() != nil {
					t.Fatal("wrong subprocess bound")
				}
				calls = append(calls, program)
				if program == "/usr/bin/defaults" {
					if len(args) != 2 || args[0] != "read" || args[1] != plist {
						t.Fatal(args)
					}
					return ProcessResult{OK: defaultsOK, Stdout: []byte("{\n CFBundleIdentifier = \"com.example.App\";\n CFBundleShortVersionString = 1.2.3;\n}")}, true
				}
				if len(args) != 6 || args[0] != "-extract" || args[2] != "raw" || args[3] != "-o" || args[4] != "-" || args[5] != plist {
					t.Fatal(args)
				}
				value := "com.example.App"
				if args[1] == "CFBundleShortVersionString" {
					value = "1.2.3"
				}
				return ProcessResult{OK: true, Stdout: []byte(value + "\n")}, true
			}
			id, version := macReadBundleMetadata(context.Background(), bundle, runner)
			if id == nil || *id != "com.example.App" || version == nil || *version != "1.2.3" {
				t.Fatal(id, version)
			}
			want := 1
			if !defaultsOK {
				want = 3
			}
			if len(calls) != want {
				t.Fatal(calls)
			}
		})
	}
	if macDefaultsValue("NotCFBundleIdentifier = wrong;", "CFBundleIdentifier") != nil || macDefaultsValue("CFBundleIdentifier = \"\";", "CFBundleIdentifier") != nil {
		t.Fatal("key matching")
	}
	id, version := macReadBundleMetadata(context.Background(), bundle, func(context.Context, time.Duration, string, ...string) (ProcessResult, bool) {
		return ProcessResult{}, false
	})
	if id != nil || version != nil {
		t.Fatal("transport failure is not metadata")
	}
}

func TestMacBundleIDsExactFallbackAndUnknownRound(t *testing.T) {
	paths := []string{filepath.Join("apps", "Indexed.app"), filepath.Join("private", "Hidden.app")}
	read := func(path string) *string {
		if path != filepath.Join(paths[1], "Contents", "Info.plist") {
			t.Fatalf("fallback on wrong bundle: %s", path)
		}
		id := "com.example.Hidden"
		return &id
	}
	for _, output := range []string{"com.example.Indexed\x00(null)", "com.example.Indexed\x00(null)\x00"} {
		ids, ok := macIDsFromMetadata(paths, []byte(output), read)
		if !ok || len(ids) != 2 {
			t.Fatal(ids, ok)
		}
		if _, ok := ids["com.example.Hidden"]; !ok {
			t.Fatal(ids)
		}
	}
	for _, bad := range []string{"", " ", "(null)"} {
		ids, ok := macIDsFromMetadata(paths, []byte("com.example.Indexed\x00(null)"), func(string) *string { return &bad })
		if ok || ids != nil {
			t.Fatalf("partial set escaped: %v", ids)
		}
	}
	for _, output := range []string{"com.example.Indexed", "a\x00b\x00c", "a\x00\xff"} {
		if ids, ok := macIDsFromMetadata(paths, []byte(output), read); ok || ids != nil {
			t.Fatal("malformed metadata", output)
		}
	}
	if ids, ok := macIDsFromMetadata(paths, []byte("a\x00b"), func(string) *string { t.Fatal("valid metadata needs no plist"); return nil }); !ok || len(ids) != 2 {
		t.Fatal(ids)
	}
}

func TestMacBundleQueryQuotesAndDU(t *testing.T) {
	for id, want := range map[string]string{"com.example.App": `kMDItemCFBundleIdentifier == "com.example.App"`, `a\b"c`: `kMDItemCFBundleIdentifier == "a\\b\"c"`} {
		if got := macBundleQuery(id); got != want {
			t.Fatalf("%q != %q", got, want)
		}
	}
	sizes := macParseDU([]byte("4\t/Applications/Space App.app\n18446744073709551615\t/max.app\n-1\t/bad.app\ninvalid\n"))
	if sizes["/Applications/Space App.app"] != 4096 || sizes["/max.app"] != math.MaxUint64 || len(sizes) != 2 {
		t.Fatal(sizes)
	}
}

func TestMacBundleRealBundlesAndGenericUninstaller(t *testing.T) {
	root := t.TempDir()
	bundle := macFixtureBundle(t, root, "Main.app")
	if !macRealBundle(bundle) || macRealBundle(filepath.Join(root, "cache.app")) {
		t.Fatal("plist file required")
	}
	if err := os.MkdirAll(filepath.Join(root, "directory.app", "Contents", "Info.plist"), 0700); err != nil {
		t.Fatal(err)
	}
	if macRealBundle(filepath.Join(root, "directory.app")) {
		t.Fatal("plist directory is not a bundle")
	}
	for _, name := range []string{"Uninstall notes.txt", "Other.app", "uninstall helper.app"} {
		if err := os.MkdirAll(filepath.Join(bundle, "Contents", "Resources", name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	got := macGenericUninstaller(bundle)
	if got == nil || filepath.Base(*got) != "uninstall helper.app" {
		t.Fatal(got)
	}
}

func TestMacBundleFallbackUsesNewestUserSignal(t *testing.T) {
	home := t.TempDir()
	id := "com.example.App"
	for i, rel := range []string{"Preferences/" + id + ".plist", "WebKit/" + id, "Containers/" + id} {
		path := filepath.Join(home, "Library", rel)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, nil, 0600); err != nil {
			t.Fatal(err)
		}
		stamp := time.Unix(1000+int64(i)*1000, 0)
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	if got := macFallbackLastUsed(home, id); got.raw != 3000 || got.date == nil {
		t.Fatal(got)
	}
	if macFallbackLastUsed(home, "absent").raw != 0 {
		t.Fatal("missing data fabricated a date")
	}
}

func TestMacBundleRegistrationEvidence(t *testing.T) {
	root := t.TempDir()
	app := macFixtureBundle(t, root, "Hidden (test).app")
	id := "com.example.Hidden"
	text := "bundle id: Hidden (0x1)\npath: " + app + " (0xa)\nidentifier: " + id + "\n"
	records, ok := macRegistrationRecords(text)
	if !ok || !reflect.DeepEqual(records[strings.ToLower(id)], []string{app}) {
		t.Fatal(records, ok)
	}
	for _, bad := range []string{"unknown output", text + strings.Repeat("-", 80) + "\nbundle id: Bad\nidentifier: com.example.bad\npath: malformed\n"} {
		if _, ok := macRegistrationRecords(bad); ok {
			t.Fatal("unknown registration accepted")
		}
	}
	read := func(string) *string { return &id }
	trash := filepath.Join(root, ".Trash")
	live, ok := macRegisteredLive(context.Background(), records, trash, read)
	if !ok || len(live) != 1 {
		t.Fatal(live, ok)
	}
	records[strings.ToLower(id)] = []string{filepath.Join(root, "stale.app")}
	live, ok = macRegisteredLive(context.Background(), records, trash, read)
	if !ok || len(live) != 0 {
		t.Fatal("stale path is not live or removal evidence", live, ok)
	}
	trashed := macFixtureBundle(t, trash, "Hidden.app")
	records[strings.ToLower(id)] = []string{trashed}
	live, ok = macRegisteredLive(context.Background(), records, trash, read)
	if !ok || len(live) != 0 {
		t.Fatal(live, ok)
	}
	records[strings.ToLower(id)] = []string{app}
	if _, ok = macRegisteredLive(context.Background(), records, trash, func(string) *string { return nil }); ok {
		t.Fatal("unknown identity must refuse whole round")
	}
	wrong := "com.example.Other"
	if _, ok = macRegisteredLive(context.Background(), records, trash, func(string) *string { return &wrong }); ok {
		t.Fatal("mismatched identity accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, ok = macRegisteredLive(ctx, records, trash, read); ok {
		t.Fatal("cancelled probe")
	}
	alias := filepath.Join(trash, "Alias.app")
	if err := os.Symlink(app, alias); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	records[strings.ToLower(id)] = []string{alias}
	live, ok = macRegisteredLive(context.Background(), records, trash, read)
	if !ok || len(live) != 1 {
		t.Fatal("Trash symlink must preserve live target", live, ok)
	}
}

// 从 DMG 拖进 Applications 之后，用户常把安装盘一直挂着。挂载卷里的那份副本
// 会被 LaunchServices 登记，但它不是安装：把它当成「仍安装」，刚刚删掉主程序
// 的用户就永远清不掉残留。判据是卷身份——系统数据卷以外的副本不算安装。
func TestMacRegisteredLiveIgnoresBundlesOnOtherVolumes(t *testing.T) {
	root := t.TempDir()
	id := "com.example.Dmg"
	app := macFixtureBundle(t, root, "Dmg.app")
	records := map[string][]string{strings.ToLower(id): {app}}
	trash := filepath.Join(root, ".Trash")
	read := func(string) *string { return &id }

	// 夹具在系统临时目录（数据卷），默认按安装计。
	live, ok := macRegisteredLive(context.Background(), records, trash, read)
	if !ok || len(live) != 1 {
		t.Fatalf("same-volume bundle must stay live: %v %v", live, ok)
	}

	// 同一份记录，一旦卷身份说明它不在本机数据卷上，就不该再算安装。
	sameVolume := func(string) (bool, bool) { return false, true }
	live, ok = macRegisteredLive(context.Background(), records, trash, read, sameVolume)
	if !ok || len(live) != 0 {
		t.Fatalf("mounted image copy counted as an installation: %v %v", live, ok)
	}
	// 卷身份测不出时不能当作「不在本机」——那是 fail open，方向错在放行删除。
	live, ok = macRegisteredLive(context.Background(), records, trash, read, func(string) (bool, bool) { return false, false })
	if !ok || len(live) != 1 {
		t.Fatalf("unknown volume identity must not grant removal: %v %v", live, ok)
	}
}

// Spotlight 会索引不是安装的 bundle：应用做 bundle 迁移时会在
// ~/Library/Caches/<id>.BundleMigration/backups/ 留一份完整的 `.backup.app`，
// 它带着同一个 CFBundleIdentifier。把这份缓存当成「仍安装」，刚卸载完的应用
// 就永远确认不了卸载完成，残留清理被永久拒绝。
func TestMacInstalledLocationRejectsCachedBundleCopies(t *testing.T) {
	home := t.TempDir()
	installed := macFixtureBundle(t, filepath.Join(home, "Applications"), "App.app")
	cached := macFixtureBundle(t,
		filepath.Join(home, "Library", "Caches", "com.example.app.BundleMigration", "backups"),
		"App-1.2.3.backup.app")
	// 用户自己选的非标准安装位置不能被误伤。
	odd := macFixtureBundle(t, filepath.Join(home, "Desktop", "便携版"), "App.app")

	if !macInstalledLocation(installed, home) {
		t.Fatal("Applications install rejected")
	}
	if macInstalledLocation(cached, home) {
		t.Fatal("cached bundle copy counted as an installation")
	}
	if !macInstalledLocation(odd, home) {
		t.Fatal("non-standard install location rejected")
	}
	// 不是 app bundle 的路径本来就不算安装。
	if macInstalledLocation(filepath.Join(home, "Library", "Caches", "plain"), home) {
		t.Fatal("non-bundle path accepted")
	}
	// 废纸篓里的 bundle 不是安装：Spotlight 滞后索引会把拖进废纸篓的 .app
	// 继续报出来，不认掉它，「卸载残留检测」的复核就会把卸载当成没发生。
	trashed := macFixtureBundle(t, filepath.Join(home, ".Trash"), "App.app")
	if macInstalledLocation(trashed, home) {
		t.Fatal("trashed bundle counted as an installation")
	}
	// 根目录都给不出时保持保守：无法判断就别宣称它不在本机。
	if !macInstalledLocation(installed, "") {
		t.Fatal("missing home must not reject a real bundle")
	}
}

// mdfind 的输出里既有真安装也可能只有缓存副本：只挑能装应用的路径。
func TestMacAnyInstallationSkipsCachedCopies(t *testing.T) {
	home := t.TempDir()
	installed := macFixtureBundle(t, filepath.Join(home, "Applications"), "App.app")
	cached := macFixtureBundle(t,
		filepath.Join(home, "Library", "Caches", "com.example.app.BundleMigration", "backups"),
		"App-1.2.3.backup.app")

	if !macAnyInstallation(installed, home) {
		t.Fatal("real installation not recognised")
	}
	if macAnyInstallation(cached, home) {
		t.Fatal("only a cached copy: the app must read as uninstalled")
	}
	if macAnyInstallation(installed+"\n"+cached, home) != true {
		t.Fatal("one real installation is enough")
	}
	if macAnyInstallation("", home) {
		t.Fatal("empty output is not an installation")
	}
}

// LaunchServices 也会登记缓存里的 bundle 备份：应用迁移 bundle 时留下
// `~/Library/Caches/<id>.BundleMigration/backups/*.backup.app`，它带着同一个
// bundle id。把它当成「仍安装」，刚卸载完的应用永远确认不了卸载完成。
func TestMacRegisteredLiveIgnoresCachedBundleCopies(t *testing.T) {
	home := t.TempDir()
	id := "com.example.migrated"
	backup := macFixtureBundle(t,
		filepath.Join(home, "Library", "Caches", "com.example.migrated.BundleMigration", "backups"),
		"App-1.2.3.backup.app")
	records := map[string][]string{id: {backup}}
	trash := filepath.Join(home, ".Trash")
	read := func(string) *string { return &id }
	sameVolume := func(string) (bool, bool) { return true, true }

	live, ok := macRegisteredLive(context.Background(), records, trash, read, sameVolume)
	if !ok || len(live) != 0 {
		t.Fatalf("cached bundle backup counted as a live installation: %v %v", live, ok)
	}
	// 真正装在 Applications 的那份仍然要算。
	installed := macFixtureBundle(t, filepath.Join(home, "Applications"), "App.app")
	records[id] = []string{installed}
	live, ok = macRegisteredLive(context.Background(), records, trash, read, sameVolume)
	if !ok || len(live) != 1 {
		t.Fatalf("real installation lost: %v %v", live, ok)
	}
}

func TestMacSpotlightInstallationFiltersOtherVolumes(t *testing.T) {
	home := t.TempDir()
	bundle := macFixtureBundle(t, home, "Mounted.app")
	if macAnyInstallation(bundle, home, func(string) (bool, bool) { return false, true }) {
		t.Fatal("Spotlight mounted-volume copy counted as an installation")
	}
	for _, probe := range []macVolumeProbe{
		func(string) (bool, bool) { return true, true },
		func(string) (bool, bool) { return false, false },
	} {
		if !macAnyInstallation(bundle, home, probe) {
			t.Fatal("local or unknown-volume bundle must remain installed")
		}
	}
}
