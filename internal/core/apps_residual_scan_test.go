package core

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func macResidualFixture(t *testing.T) (macResidualRoots, *RuleSnapshot) {
	t.Helper()
	root := t.TempDir()
	roots := macResidualRoots{
		home: filepath.Join(root, "home"), systemLibrary: filepath.Join(root, "Library"),
		receipts: filepath.Join(root, "receipts"), darwinCache: filepath.Join(root, "DarwinCache"),
		darwinTemp: filepath.Join(root, "DarwinTemp"), applications: filepath.Join(root, "Applications"),
	}
	for _, path := range []string{roots.home, roots.systemLibrary, roots.receipts, roots.darwinCache, roots.darwinTemp, roots.applications} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	return roots, snapshot
}

func TestMacResidualScanComponentsSatellitesAndManualData(t *testing.T) {
	roots, snapshot := macResidualFixture(t)
	appPath := filepath.Join(roots.applications, "Fixture.app")
	app := InstalledApp{ID: "fixture", Name: "Fixture Editor", RegistrySubpath: "com.google.Fixture", InstallLocation: &appPath}
	components := map[string]string{
		"Contents/XPCServices/Worker.xpc/Contents/Info.plist":                   "net.component.worker",
		"Contents/PlugIns/Helper.appex/Contents/Info.plist":                     "net.component.appex",
		"Contents/Library/LoginItems/Login.app/Contents/Info.plist":             "net.component.login",
		"Contents/Frameworks/Sparkle.framework/Updater.xpc/Contents/Info.plist": "org.sparkle-project.Updater",
		"Contents/Resources/Foreign.app/Contents/Info.plist":                    "net.foreign.app",
	}
	metadata := map[string]string{}
	for relative, id := range components {
		path := filepath.Join(appPath, filepath.FromSlash(relative))
		sourceFixtureFile(t, path, id)
		metadata[path] = id
	}
	evidence := macResidualEvidence{
		metadata: func(path string) (*string, *string) {
			if value, ok := metadata[path]; ok {
				return &value, nil
			}
			return nil, nil
		},
		groups: []string{"group.shared.signed", "../escape"},
	}
	uuid := "5E9D1F0A-1B2C-3D4E-5F60-123456789ABC"
	want := map[string]struct {
		confidence Confidence
		manual     bool
	}{
		"Preferences/com.google.Fixture.plist":                                {Certain, false},
		"Caches/com.google.Fixture.helper.GPU":                                {Certain, false},
		"Preferences/ByHost/com.google.Fixture." + uuid + ".plist":            {Certain, false},
		"Caches/com.google.Fixture.ShipIt." + uuid + ".plist":                 {Certain, false},
		"Caches/net.component.worker":                                         {Certain, false},
		"Caches/net.component.appex":                                          {Certain, false},
		"Caches/net.component.login":                                          {Certain, false},
		"Containers/com.google.Fixture":                                       {Certain, true},
		"Group Containers/group.shared.signed":                                {Certain, true},
		"Application Scripts/group.shared.signed":                             {Certain, true},
		"Application Scripts/com.google.Fixture":                              {Certain, true},
		"WebKit/com.google.Fixture":                                           {Possible, false},
		"Caches/FixtureEditor":                                                {Certain, false},
		"Application Support/CrashReporter/Fixture Editor_" + uuid + ".plist": {Certain, false},
	}
	for relative := range want {
		path := filepath.Join(roots.home, "Library", filepath.FromSlash(relative))
		if filepath.Ext(path) == ".plist" {
			sourceFixtureFile(t, path, "metadata")
		} else {
			sourceFixtureFile(t, filepath.Join(path, "payload"), "owned bytes")
		}
	}
	for _, relative := range []string{
		"Caches/com.google.Fixture.cn", "Caches/com.google.Fixture.helper.fake", "Caches/net.foreign.app",
		"Caches/org.sparkle-project.Updater", "Group Containers/group.other.unsigned", "WebKit/com.google.Other",
		"Application Support/CrashReporter/Fixture EditorMonitor_" + uuid + ".plist",
		"Application Support/CrashReporter/Fixture Editor_bad.plist",
	} {
		sourceFixtureFile(t, filepath.Join(roots.home, "Library", filepath.FromSlash(relative)), "unowned")
	}
	result, err := scanMacAppResidualsIn(context.Background(), app, roots, evidence, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	if result.AppName != app.Name || result.AppID != app.ID || len(result.Items) != len(want) {
		t.Fatalf("unexpected scan metadata/count: %+v", result)
	}
	var size uint64
	for _, item := range result.Items {
		relative, _ := filepath.Rel(filepath.Join(roots.home, "Library"), item.Kind.Path)
		expect, ok := want[filepath.ToSlash(relative)]
		if !ok || item.Confidence != expect.confidence || item.RequiresConfirmation != expect.manual || item.Identity == nil || item.Rule == nil {
			t.Fatalf("incorrect ownership/default/evidence: %s %+v", relative, item)
		}
		size += item.Size()
		delete(want, filepath.ToSlash(relative))
	}
	if len(want) != 0 || size == 0 || result.TotalFileSize != size {
		t.Fatalf("missing=%v total=%d sum=%d", want, result.TotalFileSize, size)
	}
	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	result, err = scanMacAppResidualsIn(canceled, app, roots, evidence, snapshot)
	if !errors.Is(err, context.Canceled) || len(result.Items) != 0 {
		t.Fatalf("canceled scan published candidates: %+v %v", result, err)
	}
}

func TestMacResidualReviewedVersionsAndManifestOwnership(t *testing.T) {
	roots, snapshot := macResidualFixture(t)
	appPath := filepath.Join(roots.applications, "LyX.app")
	executable := filepath.Join(appPath, "Contents/MacOS/native-host")
	sourceFixtureFile(t, executable, "host")
	app := InstalledApp{Name: "Document Editor", RegistrySubpath: "org.lyx.lyx", InstallLocation: &appPath}
	library := filepath.Join(roots.home, "Library")
	for _, path := range []string{"Application Support/LyX-2.4", "Application Support/LyX-2.4.1", "Application Support/LyX-", "Application Support/LyX-2.beta", "Application Support/LyX-2..4", "Application Support/LyX-2.4-other"} {
		sourceFixtureFile(t, filepath.Join(library, path, "data"), "version")
	}
	for _, root := range []string{library, roots.systemLibrary} {
		manifests := filepath.Join(root, "Application Support/Google/Chrome/NativeMessagingHosts")
		for name, target := range map[string]string{
			"owned.json": executable, "foreign.json": filepath.Join(roots.home, "foreign"),
			"missing.json": filepath.Join(appPath, "Contents/MacOS/missing"), "directory.json": filepath.Dir(executable),
		} {
			data, _ := json.Marshal(map[string]string{"path": target})
			sourceFixtureFile(t, filepath.Join(manifests, name), string(data))
		}
	}
	sourceFixtureFile(t, filepath.Join(roots.home, "foreign"), "outside")
	result, err := scanMacAppResidualsIn(context.Background(), app, roots, macResidualEvidence{}, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, item := range result.Items {
		if item.Confidence != Possible || item.Identity == nil || item.Rule == nil {
			t.Fatalf("review must remain opt-in and identity-bound: %+v", item)
		}
		names = append(names, filepath.Base(item.Kind.Path))
	}
	slices.Sort(names)
	if !slices.Equal(names, []string{"LyX-2.4", "LyX-2.4.1", "owned.json", "owned.json"}) {
		t.Fatalf("versions/manifests=%q", names)
	}
	// An escaping manifest symlink cannot inherit the application's ownership.
	link := filepath.Join(library, "Application Support/Google/Chrome/NativeMessagingHosts/linked.json")
	if err := os.Symlink(filepath.Join(roots.systemLibrary, "Application Support/Google/Chrome/NativeMessagingHosts/owned.json"), link); err == nil {
		result, err = scanMacAppResidualsIn(context.Background(), app, roots, macResidualEvidence{}, snapshot)
		if err != nil || slices.ContainsFunc(result.Items, func(item ResidualItem) bool { return item.Kind.Path == link }) {
			t.Fatalf("redirected manifest was claimed: %v %+v", err, result.Items)
		}
	}
}

func TestMacResidualDotdirAmbiguityAndManualAppSupport(t *testing.T) {
	roots, snapshot := macResidualFixture(t)
	app := InstalledApp{Name: "Qoder CN", RegistrySubpath: "com.qoder.cn"}
	short := filepath.Join(roots.home, ".qoder")
	exact := filepath.Join(roots.home, ".qoder-cn")
	sourceFixtureFile(t, filepath.Join(short, "data"), "shared CLI")
	scan := func() []string {
		t.Helper()
		result, err := scanMacAppResidualsIn(context.Background(), app, roots, macResidualEvidence{}, snapshot)
		if err != nil {
			t.Fatal(err)
		}
		var paths []string
		for _, item := range result.Items {
			if item.Source == ResidualDotConfigDir {
				if item.Confidence != Possible {
					t.Fatal("dotdir gained certainty")
				}
				paths = append(paths, item.Kind.Path)
			}
		}
		return paths
	}
	if paths := scan(); !slices.Equal(paths, []string{short}) {
		t.Fatalf("single weak family should be offered only for review: %q", paths)
	}
	other := filepath.Join(roots.applications, "QoderWork CN.app")
	if err := os.Mkdir(other, 0700); err != nil {
		t.Fatal(err)
	}
	if paths := scan(); len(paths) != 0 {
		t.Fatalf("another installed product owns the ambiguous family: %q", paths)
	}
	if err := os.Remove(other); err != nil {
		t.Fatal(err)
	}
	sourceFixtureFile(t, filepath.Join(exact, "data"), "exclusive GUI")
	if paths := scan(); !slices.Equal(paths, []string{exact}) {
		t.Fatalf("exact sibling must veto shorter CLI directory: %q", paths)
	}
	app = InstalledApp{Name: "青简", RegistrySubpath: "app.qingjian.inputmethod"}
	support := filepath.Join(roots.home, "Library/Application Support", app.Name)
	sourceFixtureFile(t, filepath.Join(support, "dictionary"), "user data")
	result, err := scanMacAppResidualsIn(context.Background(), app, roots, macResidualEvidence{}, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	if !slices.ContainsFunc(result.Items, func(i ResidualItem) bool {
		return i.Kind.Path == support && i.Confidence == Certain && i.RequiresConfirmation
	}) {
		t.Fatalf("reviewed user dictionary must require confirmation: %+v", result.Items)
	}
}

func TestMacResidualSystemExtensionsExactVersusFamily(t *testing.T) {
	roots, snapshot := macResidualFixture(t)
	app := InstalledApp{Name: "Karabiner Elements", RegistrySubpath: "org.pqrs.Karabiner.Settings"}
	evidence := macResidualEvidence{extensions: parseSystemExtensions("*\t*\tTEAM\torg.pqrs.Karabiner.Settings (1)\tname\t[activated enabled]\n*\t*\tTEAM\torg.pqrs.Driver (1)\tname\t[activated enabled]\n*\t*\tOTHER\torg.pqrsx.Driver (1)\tname\t[activated enabled]\n")}
	result, err := scanMacAppResidualsIn(context.Background(), app, roots, evidence, snapshot)
	if err != nil || len(result.Items) != 2 {
		t.Fatalf("extension candidates=%+v err=%v", result.Items, err)
	}
	for _, item := range result.Items {
		want := Possible
		if item.Kind.BundleID == app.RegistrySubpath {
			want = Certain
		}
		if item.Confidence != want || !item.RequiresConfirmation || item.Kind.Type != ResidualSystemExtension || item.Size() != 0 {
			t.Fatalf("extension authority/default=%+v", item)
		}
	}
}

func TestMacResidualVerificationRetainsUnknownAndRefreshesIdentity(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "data")
	sourceFixtureFile(t, path, "before uninstall")
	item := CertainResidual(ResidualKind{Type: ResidualFile, Path: path, Bytes: 100000}, ResidualAppSupportDir)
	item.RequiresConfirmation = true
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	sourceFixtureFile(t, path, "new object")
	extension := CertainResidual(ResidualKind{Type: ResidualSystemExtension, BundleID: "com.example.driver"}, ResidualExtensionSource)
	missing := CertainResidual(ResidualKind{Type: ResidualFile, Path: filepath.Join(root, "missing"), Bytes: 123}, ResidualOther)
	remaining, err := verifyMacResidualsWith(context.Background(), []ResidualItem{item, missing, extension}, nil, false)
	if err != nil || len(remaining) != 2 || remaining[0].Identity == nil || !remaining[0].Identity.Recheck(path) || item.Identity.Recheck(path) || !remaining[0].RequiresConfirmation || remaining[0].Size() >= item.Size() {
		t.Fatalf("refresh must bind new object without losing manual intent: %+v err=%v", remaining, err)
	}
	remaining, err = verifyMacResidualsWith(context.Background(), []ResidualItem{extension}, nil, true)
	if err != nil || len(remaining) != 0 {
		t.Fatalf("known empty extension inventory=%+v %v", remaining, err)
	}
	remaining, err = verifyMacResidualsWith(context.Background(), []ResidualItem{extension}, []ResidualKind{{Type: ResidualSystemExtension, BundleID: "COM.EXAMPLE.DRIVER"}}, true)
	if err != nil || len(remaining) != 1 {
		t.Fatalf("case-insensitive active extension=%+v %v", remaining, err)
	}
	t.Run("unknown-and-dangling", func(t *testing.T) {
		loop := filepath.Join(root, "loop")
		if err := os.Symlink(loop, loop); err != nil {
			t.Skipf("symlinks unavailable: %v", err)
		}
		dangling := filepath.Join(root, "dangling")
		if err := os.Symlink(filepath.Join(root, "missing"), dangling); err != nil {
			t.Fatal(err)
		}
		unknown := filepath.Join(loop, "state")
		if _, err := os.Lstat(unknown); err == nil || os.IsNotExist(err) {
			t.Fatalf("fixture must be an unknown read, not absence: %v", err)
		}
		items := []ResidualItem{
			PossibleResidual(ResidualKind{Type: ResidualFile, Path: unknown}, ResidualOther),
			PossibleResidual(ResidualKind{Type: ResidualFile, Path: dangling}, ResidualOther),
			missing,
		}
		remaining, err := verifyMacResidualsWith(context.Background(), items, nil, true)
		if err != nil || len(remaining) != 2 || remaining[0].Kind.Path != unknown || remaining[1].Kind.Path != dangling || remaining[0].Identity != nil || remaining[1].Identity == nil || !remaining[1].Identity.Recheck(dangling) {
			t.Fatalf("retain unknown evidence and capture the link itself, not its target: %+v %v", remaining, err)
		}
	})
}

func TestMacResidualScanDotAtomicTempFiles(t *testing.T) {
	roots, snapshot := macResidualFixture(t)
	app := InstalledApp{ID: "lobster", Name: "LobsterAI", RegistrySubpath: "com.lobsterai.app"}
	dotCache := filepath.Join(roots.darwinCache, ".com.lobsterai.app.e08PpG")
	if err := os.WriteFile(dotCache, []byte("cache chunk"), 0o600); err != nil {
		t.Fatal(err)
	}
	dotTemp := filepath.Join(roots.darwinTemp, ".com.lobsterai.app.ixL6vL")
	if err := os.WriteFile(dotTemp, []byte("temp chunk"), 0o600); err != nil {
		t.Fatal(err)
	}
	unrelated := filepath.Join(roots.darwinTemp, ".bun-501-c22f2ef692948d9e")
	if err := os.WriteFile(unrelated, []byte("other app"), 0o600); err != nil {
		t.Fatal(err)
	}

	result, err := scanMacAppResidualsIn(context.Background(), app, roots, macResidualEvidence{}, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	paths := make(map[string]bool)
	for _, it := range result.Items {
		paths[it.Kind.Path] = true
	}
	if !paths[dotCache] {
		t.Fatalf("expected atomic temp file in darwinCache %q to be included in residuals", dotCache)
	}
	if !paths[dotTemp] {
		t.Fatalf("expected atomic temp file in darwinTemp %q to be included in residuals", dotTemp)
	}
	if paths[unrelated] {
		t.Fatalf("unrelated dotfile %q must not be included", unrelated)
	}
}

func TestMacAtomicTempSuffixDoesNotClaimSiblingProducts(t *testing.T) {
	for _, name := range []string{"com.example.app.editor", "com.example.app.client.plist", "com.example.app.helper.editor"} {
		if macBundleSatellite(name, "com.example.app", []string{"helper"}) {
			t.Errorf("sibling product claimed as atomic temp: %s", name)
		}
	}
	for _, name := range []string{".com.example.app.e08PpG", ".com.example.app.helper.ixL6vL"} {
		if !macBundleSatellite(name, "com.example.app", []string{"helper"}) {
			t.Errorf("hidden atomic temp omitted: %s", name)
		}
	}
}
