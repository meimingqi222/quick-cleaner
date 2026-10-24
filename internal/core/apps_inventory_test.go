package core

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestAppInventoryEnrichmentAndRegistrationMerge(t *testing.T) {
	root := t.TempDir()
	shallow := time.Date(2000, 1, 2, 3, 4, 5, 0, time.UTC)
	deep := time.Date(2035, 2, 3, 4, 5, 6, 0, time.UTC)
	for path, body := range map[string]string{"app.exe": "1234567", "one/a.dat": "12345678901", "one/two/b.dat": "1234567890123"} {
		path = filepath.Join(root, path)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
		stamp := shallow
		if filepath.Base(path) == "b.dat" {
			stamp = deep
		}
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	command, date := "powershell -Command official", "2020-04-05"
	icon := `"` + filepath.Join(root, "app.exe") + `",-1`
	apps := []InstalledApp{
		{ID: "first", Name: "Custom Tool", Version: "1", RegistryRoot: AppHKLM},
		{ID: "second", Name: "CUSTOM TOOL", Version: "1", RegistryRoot: AppHKCU, InstallLocation: &root, DisplayIcon: &icon, UninstallString: &command, EstimatedSize: 3, InstallDate: &date, InstallDateRaw: 20200405},
	}
	var activity platform.AppActivityIndex
	activity.Insert("custom", 1100000000)
	activity.Insert("app.exe", 1150000000)
	result := EnrichWindowsAppInventory(context.Background(), apps, activity, map[string]uint64{"tool": 1120000000})
	if len(result) != 1 {
		t.Fatalf("same name/version registrations not merged: %+v", result)
	}
	got := result[0]
	if got.ID != "first" || got.RegistryRoot != AppHKLM || got.EstimatedSize != 31 || got.UninstallerMissing || got.InstallDateRaw != 20200405 || got.InstallDate == nil || *got.InstallDate != date {
		t.Fatalf("registration precedence/measurement changed: %+v", got)
	}
	if got.LastUsedRaw != 1150000000 {
		t.Fatalf("must include icon execution and exclude deep atime: got %d", got.LastUsedRaw)
	}
	if apps[0].EstimatedSize != 0 || apps[0].InstallLocation != nil {
		t.Fatal("enrichment mutated source records")
	}
}

func TestAppInventorySharedLocationsKeepDiscoveredInstances(t *testing.T) {
	root := filepath.Join(t.TempDir(), "missing")
	command := "msiexec /x {id}"
	apps := []InstalledApp{
		{ID: "v10", Name: "App", Version: "10", InstallLocation: &root, EstimatedSize: 44, UninstallString: &command},
		{ID: "v9", Name: "App", Version: "9", InstallLocation: &root, UninstallString: &command},
		{ID: "portable-one", Name: "App", Version: "9", InstallLocation: &root, EstimatedSize: 13, Discovery: &AppDiscovery{}},
		{ID: "portable-two", Name: "App", Version: "9", InstallLocation: &root, EstimatedSize: 17, Discovery: &AppDiscovery{}},
	}
	result := EnrichWindowsAppInventory(context.Background(), apps, platform.AppActivityIndex{}, nil)
	if len(result) != 3 {
		t.Fatalf("discovered installations must remain distinct: %+v", result)
	}
	byID := map[string]InstalledApp{}
	for _, app := range result {
		byID[app.ID] = app
	}
	if byID["v9"].EstimatedSize != 44 || byID["portable-one"].EstimatedSize != 13 || byID["portable-two"].EstimatedSize != 17 {
		t.Fatalf("reference lexical-version winner and size fallback changed: %+v", result)
	}
	if byID["portable-one"].CanUninstall() {
		t.Fatal("discovery without official policy must not grant an uninstall command")
	}
}

func TestAppInventoryUninstallerAvailability(t *testing.T) {
	root := t.TempDir()
	missing := `"` + filepath.Join(root, "missing.exe") + `" /S`
	quiet := "winget uninstall exact"
	msi := "msiexec /X {id}"
	apps := []InstalledApp{
		{ID: "absent", Name: "Absent", UninstallString: &missing},
		{ID: "quiet", Name: "Quiet", UninstallString: &missing, QuietUninstallString: &quiet},
		{ID: "msi", Name: "MSI", UninstallString: &msi},
		{ID: "none", Name: "None"},
	}
	result := EnrichWindowsAppInventory(context.Background(), apps, platform.AppActivityIndex{}, nil)
	for _, app := range result {
		want := app.ID == "absent" || app.ID == "none"
		if app.UninstallerMissing != want {
			t.Fatalf("%s missing=%v, want %v", app.ID, app.UninstallerMissing, want)
		}
	}
}

func TestAppInventoryDeducesIconButDoesNotExpandDiscovery(t *testing.T) {
	root := t.TempDir()
	executable := filepath.Join(root, "launcher.exe")
	if err := os.WriteFile(executable, []byte("exe"), 0600); err != nil {
		t.Fatal(err)
	}
	icon := `"` + executable + `",-4`
	app := InstalledApp{Name: "Unique inventory fixture", DisplayIcon: &icon}
	if location := DeduceInstallLocation(app); location == nil || *location != root {
		t.Fatalf("icon directory not inferred: %v", location)
	}
	app.Discovery = &AppDiscovery{Executable: executable}
	if location := DeduceInstallLocation(app); location != nil {
		t.Fatalf("shortcut/portable executable must not authorize its parent: %s", *location)
	}
}
