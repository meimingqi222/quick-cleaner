package core

import (
	"context"
	"encoding/binary"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"unicode/utf16"
)

type windowsResidualFixture struct {
	subs      map[string][]string
	values    map[string]map[string]string
	keys      map[string]bool
	shortcuts map[string]string
}

func newWindowsResidualFixture() *windowsResidualFixture {
	return &windowsResidualFixture{map[string][]string{}, map[string]map[string]string{}, map[string]bool{}, map[string]string{}}
}
func (f *windowsResidualFixture) KeyExists(root, path string) bool   { return f.keys[root+":"+path] }
func (f *windowsResidualFixture) Subkeys(root, path string) []string { return f.subs[root+":"+path] }
func (f *windowsResidualFixture) Values(root, path string) map[string]string {
	return f.values[root+":"+path]
}
func (f *windowsResidualFixture) Value(root, path, name string) string {
	return f.values[root+":"+path][name]
}
func (f *windowsResidualFixture) Expand(text string) string {
	return asciiLower(strings.ReplaceAll(strings.Trim(text, `"`), "/", `\`))
}
func (f *windowsResidualFixture) ShortcutTargets(path, executable string) bool {
	return f.shortcuts[path] == executable && executable != ""
}
func residualFixtureWrite(t *testing.T, path, text string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}
func windowsResidualTestContext(t *testing.T, name string) windowsResidualContext {
	t.Helper()
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	return newWindowsResidualContext(InstalledApp{Name: name}, snapshot)
}
func TestWindowsResidualNamePolicy(t *testing.T) {
	c := windowsResidualTestContext(t, "Remote Desktop Manager")
	for _, name := range []string{"Desktop", "Programs", "Public", "Remote", "Manager"} {
		if c.matches(name) {
			t.Errorf("claimed generic/short name %q", name)
		}
	}
	if !c.matches("Remote Desktop Manager 2024") {
		t.Fatal("complete name not matched")
	}
	if windowsResidualName("同花顺 v9.50") != "同花顺v950" {
		t.Fatal("CJK normalization")
	}
	for _, name := range []string{"App", "Microsoft", "青简"} {
		if windowsResidualTestContext(t, name).matchable() {
			t.Errorf("unsafe fuzzy token %q", name)
		}
	}
	if c.mentions(`C:\anything`) {
		t.Fatal("empty installation matches text")
	}
	c.install = `c:\program files\foo`
	if !c.mentions(`"C:/Program Files/Foo/app.exe" --run`) {
		t.Fatal("missing path text evidence")
	}
}

func TestWindowsResidualRegistryRoutesAndViews(t *testing.T) {
	c := windowsResidualTestContext(t, "FixtureApp")
	c.install = `c:\apps\fixture`
	c.exes = []string{"fixture.exe"}
	f := newWindowsResidualFixture()
	put := func(root, path string, values map[string]string) { f.values[root+":"+path] = values }
	app := InstalledApp{Name: "FixtureApp", RegistryRoot: AppHKLM32, RegistrySubpath: `SOFTWARE\ARP\fixture`}
	f.keys["HKLM32:"+app.RegistrySubpath] = true
	for _, root := range []AppRegRoot{AppHKCU, AppHKLM, AppHKLM32} {
		base := "SOFTWARE"
		if root == AppHKCU {
			base = "Software"
		}
		if root == AppHKLM32 {
			base += `\WOW6432Node`
		}
		f.keys[root.Label()+":"+base+`\FixtureApp`] = true
		f.subs[root.Label()+":"+base] = []string{"Vendor", "FixtureApp Extra", "WOW6432Node"}
		put(root.Label(), base+`\Vendor`, map[string]string{"install": `c:\apps\fixture\bin`})
		put(root.Label(), c.anchor("run_keys"), map[string]string{"path": `c:\apps\fixture\bin\a.exe`, "exe": `x:\other\fixture.exe --start`, "FixtureApp weak": "unrelated", "other": "unrelated"})
		if root != AppHKCU {
			anchor := c.anchor("app_paths_keys")
			f.subs[root.Label()+":"+anchor] = []string{"fixture.exe"}
		}
	}
	put("HKCU", c.anchor("user_environment_keys"), map[string]string{"EXACT_HOME": `c:\apps\fixture`, "EXACT_EXE": "fixture.exe", "FixtureApp_HOME": "unrelated", "UNRELATED": "none"})
	put("HKLM", c.anchor("user_environment_keys"), map[string]string{"EXACT_HOME": `c:\apps\fixture`})
	services := c.anchor("service_keys")
	f.subs["HKLM:"+services] = []string{"precise", "FixtureApp weak", "FixtureApp empty"}
	put("HKLM", services+`\precise`, map[string]string{"ImagePath": `c:\apps\fixture\svc.exe`})
	put("HKLM", services+`\FixtureApp weak`, map[string]string{"ImagePath": `c:\other\svc.exe`})
	put("HKLM", c.anchor("firewall_keys"), map[string]string{"owned": `App=c:\apps\fixture\app.exe`, "unrelated": "other"})
	f.subs["HKLM:"+c.anchor("tracing_keys")] = []string{"FixtureApp_RASAPI32"}
	f.subs["HKLM:"+c.anchor("heap_leak_keys")] = []string{"FIXTURE.EXE"}
	for _, anchor := range c.list("app_compat_keys") {
		for _, root := range []string{"HKCU", "HKLM"} {
			put(root, anchor, map[string]string{`c:\apps\fixture\a.exe`: "compat"})
		}
	}
	put("HKLM", c.anchor("installer_folder_keys"), map[string]string{`c:\apps\fixture\`: ""})
	put("HKCU", c.anchor("mui_cache_keys"), map[string]string{`c:\apps\fixture\a.exe`: "title"})
	put("HKLM", c.anchor("registered_apps_keys"), map[string]string{"FixtureApp": `SOFTWARE\FixtureApp\Capabilities`})
	items := scanWindowsResidualRegistry(context.Background(), app, c, f)
	want := map[ResidualSource]Confidence{ResidualUninstallEntry: Certain, ResidualConfigRegKey: Certain, ResidualLikelyConfigRegKey: Possible, ResidualVendorRegKey: Certain, ResidualAppPathsEntry: Certain, ResidualStartupEntry: Certain, ResidualUserEnvEntry: Certain, ResidualService: Certain, ResidualLikelyService: Possible, ResidualFirewallRule: Certain, ResidualRasTrace: Possible, ResidualLeakDiagnostics: Certain, ResidualCompatSetting: Certain, ResidualInstallerFolderEntry: Certain, ResidualProgramNameCache: Certain, ResidualDefaultProgramsEntry: Possible}
	for source, confidence := range want {
		if !slices.ContainsFunc(items, func(i ResidualItem) bool { return i.Source == source && i.Confidence == confidence }) {
			t.Errorf("missing %s %v", source, confidence)
		}
	}
	for _, root := range []AppRegRoot{AppHKCU, AppHKLM, AppHKLM32} {
		if !slices.ContainsFunc(items, func(i ResidualItem) bool {
			return i.Source == ResidualStartupEntry && i.Kind.Root == root && i.Kind.Name == "path"
		}) {
			t.Errorf("missing Run view %v", root)
		}
	}
	for _, item := range items {
		if item.Source == ResidualUserEnvEntry && (item.Kind.Root != AppHKCU || item.Kind.Type != ResidualRegistryValue || item.Kind.Name == "UNRELATED") {
			t.Errorf("invalid environment claim %+v", item)
		}
		if strings.HasSuffix(item.Kind.Path, "FixtureApp empty") {
			t.Fatal("empty ImagePath service claimed")
		}
		if item.Kind.Name == "FixtureApp weak" && item.Confidence != Possible {
			t.Fatal("name-only Run granted authority")
		}
	}
	changed := &RuleSnapshot{Bundle: clonePlanValue(c.snapshot.Bundle)}
	changed.Definition("residual-windows").Lists["user_environment_keys"] = nil
	c.snapshot = changed
	if slices.ContainsFunc(scanWindowsResidualRegistry(context.Background(), app, c, f), func(i ResidualItem) bool { return i.Source == ResidualUserEnvEntry }) {
		t.Fatal("empty anchor did not disable scan")
	}
}

func TestWindowsResidualCOMOwnershipAndShellLinks(t *testing.T) {
	c := windowsResidualTestContext(t, "FixtureApp")
	c.install = `c:\apps\fixture`
	f := newWindowsResidualFixture()
	guid, other := "{12345678-1234-1234-1234-1234567890AB}", "{87654321-1234-1234-1234-1234567890AB}"
	for _, base := range c.list("com_classes_roots") {
		for _, root := range []string{"HKLM", "HKCU"} {
			if root == "HKCU" {
				base = strings.Replace(base, "SOFTWARE", "Software", 1)
			}
			f.subs[root+":"+base+`\CLSID`] = []string{guid, other, "{00000000-0000-0000-0000-000000000000}"}
			f.values[root+":"+base+`\CLSID\`+guid+`\InprocServer32`] = map[string]string{"": `c:\apps\fixture\component.dll`}
			f.values[root+":"+base+`\CLSID\`+other+`\InprocServer32`] = map[string]string{"": `c:\windows\system32\other.dll`}
			f.values[root+":"+base+`\CLSID\`+guid+`\ProgID`] = map[string]string{"": "Fixture.Object"}
			f.keys[root+":"+base+`\Fixture.Object`] = true
			f.subs[root+":"+base+`\TypeLib`] = []string{guid}
			f.subs[root+":"+base+`\TypeLib\`+guid] = []string{"1.0"}
			f.values[root+":"+base+`\TypeLib\`+guid+`\1.0\0\win64`] = map[string]string{"": `c:\apps\fixture\library.tlb`}
		}
	}
	for _, root := range []string{"HKLM", "HKLM32", "HKCU"} {
		parent := c.anchor("shell_extension_parents")
		approved := c.anchor("shell_extension_approved_keys")
		if root == "HKCU" {
			parent = strings.Replace(parent, "SOFTWARE", "Software", 1)
			approved = strings.Replace(approved, "SOFTWARE", "Software", 1)
		}
		f.subs[root+":"+parent] = []string{"handler", other}
		f.values[root+":"+parent+`\handler`] = map[string]string{"": " " + strings.ToLower(guid) + " "}
		f.values[root+":"+approved] = map[string]string{guid: "name", other: "name"}
	}
	items := scanWindowsResidualCOM(context.Background(), c, f)
	for _, item := range items {
		if strings.Contains(item.Kind.Path, other) || item.Kind.Name == other {
			t.Fatal("unowned GUID claimed")
		}
	}
	for _, root := range []AppRegRoot{AppHKLM, AppHKLM32, AppHKCU} {
		if !slices.ContainsFunc(items, func(i ResidualItem) bool {
			return i.Source == ResidualShellExtension && i.Kind.Root == root && i.Kind.Type == ResidualRegistryValue
		}) {
			t.Errorf("missing approved view %v", root)
		}
	}
	if !slices.ContainsFunc(items, func(i ResidualItem) bool { return strings.HasSuffix(i.Kind.Path, "Fixture.Object") }) || !slices.ContainsFunc(items, func(i ResidualItem) bool { return strings.Contains(i.Kind.Path, "TypeLib") }) {
		t.Fatal("missing ProgID/TypeLib")
	}
}

func TestWindowsResidualScanAndPostUninstallVerification(t *testing.T) {
	root := t.TempDir()
	install := filepath.Join(root, "installed")
	exe := filepath.Join(install, "fixture.exe")
	residualFixtureWrite(t, exe, "program")
	roots := windowsResidualRoots{data: []string{filepath.Join(root, "data")}, shortcuts: []string{filepath.Join(root, "Desktop")}, local: filepath.Join(root, "local"), system: filepath.Join(root, "Windows")}
	for _, path := range []string{filepath.Join(roots.data[0], "FixtureApp", "config"), filepath.Join(roots.data[0], "FixtureApp Extra", "log"), filepath.Join(roots.shortcuts[0], "Vendor", "FixtureApp.lnk"), filepath.Join(roots.system, "Prefetch", "FIXTURE.EXE-AB12CD34.pf"), filepath.Join(roots.local, "CrashDumps", "fixture.exe.123.dmp")} {
		residualFixtureWrite(t, path, "content")
	}
	residualFixtureWrite(t, filepath.Join(roots.system, "Prefetch", "FIXTURE.EXE-NOTAHASH.pf"), "not prefetch")
	tasks := filepath.Join(roots.system, "System32", "Tasks")
	residualFixtureWrite(t, filepath.Join(tasks, "Vendor", "exact"), "<Command>"+exe+"</Command>")
	residualFixtureWrite(t, filepath.Join(tasks, "weak"), `<Command>D:\other\fixture.exe</Command>`)
	residualFixtureWrite(t, filepath.Join(tasks, "system-exe"), `<Command>C:\Windows\System32\fixture.exe</Command>`)
	residualFixtureWrite(t, filepath.Join(tasks, "Microsoft", "Windows", "excluded"), exe)
	residualFixtureWrite(t, filepath.Join(roots.shortcuts[0], "a", "b", "c", "d", "FixtureApp.lnk"), "too deep")
	f := newWindowsResidualFixture()
	app := InstalledApp{ID: "fixture", Name: "FixtureApp", InstallLocation: &install}
	result, err := scanWindowsAppResiduals(context.Background(), app, f, roots)
	if err != nil {
		t.Fatal(err)
	}
	for _, source := range []ResidualSource{ResidualInstallDir, ResidualAppDataDir, ResidualLikelyAppDataDir, ResidualShortcut, ResidualTaskSource, ResidualPrefetchFile, ResidualCrashDump} {
		if !slices.ContainsFunc(result.Items, func(i ResidualItem) bool { return i.Source == source }) {
			t.Errorf("missing %s", source)
		}
	}
	for _, item := range result.Items {
		snapshot, err := CurrentRules()
		if err != nil {
			t.Fatal(err)
		}
		if item.Rule == nil || item.Rule.ID() != "residual-windows" || item.Rule.Observation().RuleVersion != snapshot.Definition("residual-windows").Version || item.Rule.Observation().Schema == 0 {
			t.Fatal("missing complete rule observation")
		}
		if item.Kind.Path == roots.shortcuts[0] || strings.Contains(item.Kind.Path, "excluded") || strings.Contains(item.Kind.Path, "system-exe") || strings.Contains(item.Kind.Path, "NOTAHASH") || strings.Contains(Norm(item.Kind.Path), `\d\fixtureapp`) {
			t.Fatalf("escaped bounds: %+v", item)
		}
	}
	if err := os.Remove(exe); err != nil {
		t.Fatal(err)
	}
	residualFixtureWrite(t, filepath.Join(install, "leftover"), "x")
	residualFixtureWrite(t, filepath.Join(install, "late-extra"), "late")
	remaining, err := verifyWindowsAppResiduals(context.Background(), result.Items, f, roots.system)
	if err != nil {
		t.Fatal(err)
	}
	for _, item := range remaining {
		if item.Source == ResidualInstallDir {
			if item.Kind.Bytes != 5 || item.Identity != result.Items[0].Identity {
				t.Fatal("verification refreshed identity or missed new directory size")
			}
		}
	}
	if len(remaining) != len(result.Items) {
		t.Fatal("existing candidates incorrectly dropped")
	}
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := scanWindowsAppResiduals(cancelled, app, f, roots); !errors.Is(err, context.Canceled) {
		t.Fatal("scan cancellation")
	}
	if _, err := verifyWindowsAppResiduals(cancelled, result.Items, f, roots.system); !errors.Is(err, context.Canceled) {
		t.Fatal("verify cancellation")
	}
}

func TestWindowsResidualAliasRequiresCompleteRegistration(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "Qingjian")
	residualFixtureWrite(t, filepath.Join(path, "learning.log"), "data")
	const id = "{A7E3C1F2-5B94-4D6A-9C0E-2F8B1D3A6E70}_is1"
	app := InstalledApp{ID: id, Name: "青简", Publisher: "青简", Version: "0.1.0", RegistryRoot: AppHKLM, RegistrySubpath: `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\` + id}
	f := newWindowsResidualFixture()
	roots := windowsResidualRoots{data: []string{root}, system: filepath.Join(root, "Windows")}
	for _, name := range []string{"青简", "青简 0.1.0", "青简 version 0.1.0", "青简 版本 0.1.0"} {
		a := app
		a.Name = name
		result, err := scanWindowsAppResiduals(context.Background(), a, f, roots)
		if err != nil {
			t.Fatal(err)
		}
		if len(result.Items) != 1 || result.Items[0].Kind.Path != path || result.Items[0].Confidence != Possible || len(result.DefaultSelection()) != 0 {
			t.Fatalf("alias not manually selected: %+v", result)
		}
	}
	for n := range 6 {
		a := app
		switch n {
		case 0:
			a.ID = "wrong"
		case 1:
			a.Publisher = "wrong"
		case 2:
			a.Name = "青简工具"
		case 3:
			a.RegistrySubpath = "wrong"
		case 4:
			a.IsSystemComponent = true
		case 5:
			a.RegistryRoot = AppUnregistered
		}
		result, err := scanWindowsAppResiduals(context.Background(), a, f, roots)
		if err != nil {
			t.Fatal(err)
		}
		if slices.ContainsFunc(result.Items, func(i ResidualItem) bool { return i.Kind.Path == path }) {
			t.Errorf("incomplete registration %d claimed alias", n)
		}
	}
}

func TestWindowsResidualDiscoveryCannotOwnParentOrWeakNames(t *testing.T) {
	root := t.TempDir()
	exe, shortcut := filepath.Join(root, "downloads", "fixture.exe"), filepath.Join(root, "FixtureApp.lnk")
	residualFixtureWrite(t, exe, "program")
	residualFixtureWrite(t, shortcut, "shortcut")
	roots := windowsResidualRoots{data: []string{filepath.Join(root, "data")}, system: filepath.Join(root, "Windows")}
	residualFixtureWrite(t, filepath.Join(roots.data[0], "FixtureApp", "log"), "data")
	f := newWindowsResidualFixture()
	f.shortcuts[shortcut] = exe
	instance, err := CaptureInstallation(exe, []string{exe, shortcut})
	if err != nil {
		t.Fatal(err)
	}
	plan := NewCleanupPlan(EngineRuleRef(), nil)
	plan.installation = instance
	app := InstalledApp{Name: "FixtureApp", InstallLocation: &exe, Discovery: &AppDiscovery{Executable: exe, ProgramPaths: []string{exe}, Shortcuts: []string{shortcut}, Plan: plan}}
	result, err := scanWindowsAppResiduals(context.Background(), app, f, roots)
	if err != nil {
		t.Fatal(err)
	}
	for _, item := range result.Items {
		if item.Kind.Path == filepath.Dir(exe) {
			t.Fatal("portable executable inferred parent ownership")
		}
		if item.Source == ResidualAppDataDir && item.Confidence != Possible {
			t.Fatal("weak data authority")
		}
	}
	if err := os.Rename(exe, exe+".old"); err != nil {
		t.Fatal(err)
	}
	residualFixtureWrite(t, exe, "program")
	if _, err := scanWindowsAppResiduals(context.Background(), app, f, roots); err == nil {
		t.Fatal("replaced frozen artifact recaptured")
	}
}

func TestWindowsResidualTaskEncodingAndVerificationKinds(t *testing.T) {
	text := `<Command>C:\App\fixture.exe</Command>`
	for _, order := range []binary.ByteOrder{binary.LittleEndian, binary.BigEndian} {
		var data []byte
		if order == binary.LittleEndian {
			data = []byte{255, 254}
		} else {
			data = []byte{254, 255}
		}
		for _, word := range utf16.Encode([]rune(text)) {
			var pair [2]byte
			order.PutUint16(pair[:], word)
			data = append(data, pair[:]...)
		}
		if windowsResidualTaskText(data) != text {
			t.Fatal("task XML encoding")
		}
	}
	f := newWindowsResidualFixture()
	f.keys[`HKLM32:SOFTWARE\fixture`] = true
	f.values[`HKCU:Environment`] = map[string]string{"Path": "text"}
	items := []ResidualItem{CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: AppHKLM32, Path: `SOFTWARE\fixture`}, ResidualUninstallEntry), CertainResidual(ResidualKind{Type: ResidualRegistryKey, Root: AppHKLM, Path: `SOFTWARE\fixture`}, ResidualUninstallEntry), CertainResidual(ResidualKind{Type: ResidualRegistryValue, Root: AppHKCU, Path: "Environment", Name: "PATH"}, ResidualUserEnvEntry), CertainResidual(ResidualKind{Type: ResidualSystemExtension}, ResidualExtensionSource)}
	out, err := verifyWindowsAppResiduals(context.Background(), items, f, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if len(out) != 2 || out[0].Kind.Root != AppHKLM32 || out[1].Kind.Name != "PATH" {
		t.Fatalf("wrong views/kinds: %+v", out)
	}
}
