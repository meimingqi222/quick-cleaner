package core

import "testing"

func TestAppUninstallManualAndDataPolicy(t *testing.T) {
	manual := InstalledApp{RegistryRoot: AppHKCU, RegistrySubpath: "app.msime.macos.wubi"}
	if !RequiresManualUninstall(manual) {
		t.Fatal("ordinary HKCU bundle must use official settings, not direct deletion")
	}
	for _, change := range []func(*InstalledApp){
		func(a *InstalledApp) { a.RegistrySubpath += ".other" },
		func(a *InstalledApp) { a.RegistryRoot = AppUnregistered },
		func(a *InstalledApp) { a.Discovery = &AppDiscovery{} },
		func(a *InstalledApp) { a.IsSystemComponent = true },
	} {
		copy := manual
		change(&copy)
		if RequiresManualUninstall(copy) {
			t.Fatalf("manual policy widened to another identity: %+v", copy)
		}
	}
	const id = "{41454B5E-8298-42B8-BA6F-5DD2FE559E5E}_is1"
	app := InstalledApp{
		ID: id, Name: "Metasequoia IME 水杉日语 version 3.2", Publisher: "Metasequoia", Version: "3.2",
		RegistryRoot: AppHKLM32, RegistrySubpath: `Software\Microsoft\Windows\CurrentVersion\Uninstall\` + id,
	}
	if !UninstallDeletesUserData(app) {
		t.Fatal("exact registered official uninstaller must disclose data deletion")
	}
	for _, change := range []func(*InstalledApp){
		func(a *InstalledApp) { a.Name = "Metasequoia IME 水杉五笔 version 3.2" },
		func(a *InstalledApp) { a.Publisher = "metasequoia" },
		func(a *InstalledApp) { a.RegistrySubpath += "-other" },
		func(a *InstalledApp) { a.RegistryRoot = AppSystem },
		func(a *InstalledApp) { a.Discovery = &AppDiscovery{} },
	} {
		copy := app
		change(&copy)
		if UninstallDeletesUserData(copy) {
			t.Fatalf("data warning widened across registration identities: %+v", copy)
		}
	}
	if AppRegistrationMatches(InstalledApp{ID: "ſ", Name: "N", Publisher: "P", RegistryRoot: AppHKCU, RegistrySubpath: `SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\ſ`}, "s", "N", "P") {
		t.Fatal("registry matching must use ASCII case folding, not Unicode equivalence")
	}
	qingjian := InstalledApp{RegistryRoot: AppHKCU, RegistrySubpath: "app.qingjian.inputmethod"}
	if RequiresManualUninstall(qingjian) || UninstallDeletesUserData(qingjian) {
		t.Fatal("reviewed Qingjian script preserves user data and is not manual-only")
	}
}

func TestAppUninstallReviewedPolicyIsDetached(t *testing.T) {
	spec := ReviewedScriptUninstaller("app.qingjian.inputmethod")
	if spec == nil || spec.Script != "Contents/Resources/uninstall.sh" || !spec.PreservesUserData {
		t.Fatalf("reviewed script missing: %+v", spec)
	}
	spec.SHA256[0] = "changed"
	spec.SystemBundles[0] = "changed"
	if current := ReviewedScriptUninstaller(spec.BundleID); current.SHA256[0] == "changed" || current.SystemBundles[0] == "changed" {
		t.Fatal("preparing uninstall mutated the bundled policy")
	}
	if ReviewedScriptUninstaller("app.qingjian.inputmethod.other") != nil {
		t.Fatal("reviewed scripts require exact bundle identity")
	}
}
