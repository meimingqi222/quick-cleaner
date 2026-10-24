package core

import (
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestAppUninstallManualUsesNativeBundleIdentity(t *testing.T) {
	bundle := filepath.Join(t.TempDir(), "MSIME.app")
	if err := os.MkdirAll(bundle, 0700); err != nil {
		t.Fatal(err)
	}
	app := appFromRecord(platform.AppRecord{ID: "app.msime.macos.wubi", RegistrySubpath: "app.msime.macos.wubi", RegistryRoot: "HKCU", InstallLocation: &bundle})
	if app.RegistryRoot != AppHKCU || !RequiresManualUninstall(app) {
		t.Fatal("native ordinary bundle mapping lost the manual-uninstall identity")
	}
	err := runMacAppUninstallerWith(app, func(InstalledApp) (bool, error) { return true, os.RemoveAll(bundle) }, os.RemoveAll, os.RemoveAll)
	if err == nil {
		t.Fatal("internal GUI uninstall must return guidance, not success")
	}
	if _, err := os.Stat(bundle); err != nil {
		t.Fatal("manual-only application was deleted")
	}
}

func TestAppUninstallOfficialRouteFailureRetainsBundle(t *testing.T) {
	for _, route := range []string{"brew", "declared-missing", "embedded-failed"} {
		t.Run(route, func(t *testing.T) {
			bundle := filepath.Join(t.TempDir(), "App.app")
			embedded := filepath.Join(bundle, "Contents", "Frameworks", "Uninstall.app")
			if err := os.MkdirAll(embedded, 0700); err != nil {
				t.Fatal(err)
			}
			app := InstalledApp{InstallLocation: &bundle, RegistryRoot: AppHKCU, RegistrySubpath: "fixture.bundle"}
			brew := func(InstalledApp) (bool, error) { return false, nil }
			if route == "brew" {
				brew = func(InstalledApp) (bool, error) { return false, errors.New("unverifiable cask") }
			} else if route == "declared-missing" {
				app.RegistrySubpath = "us.zoom.xos"
			} else {
				app.UninstallString = &embedded
			}
			err := runMacAppUninstallerWith(app, brew, func(string) error { return errors.New("official failure") }, os.RemoveAll)
			if err == nil {
				t.Fatal("official failure must not grant success")
			}
			if _, err := os.Stat(bundle); err != nil {
				t.Fatal("official failure fell back to deleting the application")
			}
		})
	}
}

func TestAppUninstallTrashAndEmbeddedVerifyAbsence(t *testing.T) {
	bundle := filepath.Join(t.TempDir(), "App.app")
	embedded := filepath.Join(bundle, "Contents", "Resources", "Uninstall.app")
	if err := os.MkdirAll(embedded, 0700); err != nil {
		t.Fatal(err)
	}
	app := InstalledApp{InstallLocation: &bundle}
	noCask := func(InstalledApp) (bool, error) { return false, nil }
	if err := runMacAppUninstallerWith(app, noCask, os.RemoveAll, func(string) error { return nil }); err == nil {
		t.Fatal("zero-result trash must not report a still-present bundle removed")
	}
	app.UninstallString = &embedded
	if err := runMacAppUninstallerWith(app, noCask, func(string) error { return os.RemoveAll(bundle) }, func(string) error { return errors.New("trash must not be needed") }); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(bundle); !os.IsNotExist(err) {
		t.Fatal("embedded uninstall did not remove its fixture application")
	}
}
