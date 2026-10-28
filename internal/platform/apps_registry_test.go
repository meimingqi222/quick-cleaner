package platform

import (
	"strings"
	"testing"
)

func fixtureAppRegistration(text map[string]string, numbers map[string]uint32) appRegistration {
	return appRegistration{
		text: func(key string) *string {
			s, ok := text[key]
			if !ok {
				return nil
			}
			return &s
		},
		dword: func(key string) uint32 { return numbers[key] },
	}
}

func TestAppRegistrationFilters(t *testing.T) {
	for _, tc := range []struct {
		name, parent string
		component    uint32
		want         bool
	}{
		{"", "", 0, false}, {"  ", "", 0, false}, {"App", "", 1, false},
		{"KB12345", "", 0, false}, {"KB", "", 0, true}, {"kb123", "", 0, true}, {"KB工具", "", 0, true},
		{"App", "Other", 0, false}, {"Game", "sTeAm", 0, true}, {"Game", " Steam ", 0, false},
		{"App", "  ", 0, true}, {"App", "", 2, true},
	} {
		v := fixtureAppRegistration(map[string]string{"DisplayName": tc.name, "ParentKeyName": tc.parent}, map[string]uint32{"SystemComponent": tc.component})
		if _, ok := parseAppRegistration(v, "id", "HKCU", "path"); ok != tc.want {
			t.Errorf("%+v: accepted=%v", tc, ok)
		}
	}
	if _, ok := parseAppRegistration(fixtureAppRegistration(nil, nil), "", "", ""); ok {
		t.Fatal("missing name accepted")
	}
}

func TestAppRegistrationFacts(t *testing.T) {
	longCommand := `C:\Program Files (x86)\App\uninstall.exe ` + strings.Repeat("argument ", 1500)
	v := fixtureAppRegistration(map[string]string{
		"DisplayName": " App ", "DisplayVersion": " 1.2 ", "Publisher": " Vendor ",
		"InstallDate": " 20260102 ", "InstallLocation": `  "C:\App"  `,
		"DisplayIcon": ` "C:\App\app.exe",0 `, "UninstallString": " " + longCommand + " ", "QuietUninstallString": "  ",
	}, map[string]uint32{"EstimatedSize": ^uint32(0)})
	for _, root := range []string{"HKLM", "HKLM32", "HKCU"} {
		app, ok := parseAppRegistration(v, "key", root, "subpath")
		if !ok || app.Name != "App" || app.Version != "1.2" || app.Publisher != "Vendor" || app.ID != "key" || app.RegistryRoot != root || app.RegistrySubpath != "subpath" {
			t.Fatalf("facts: %+v", app)
		}
		if app.InstallDate == nil || *app.InstallDate != " 20260102 " || app.InstallDateRaw != 0 {
			t.Fatal("raw date lost")
		}
		if app.InstallLocation == nil || *app.InstallLocation != `C:\App` || app.DisplayIcon == nil || *app.DisplayIcon != `"C:\App\app.exe",0` {
			t.Fatal("path trim changed")
		}
		if app.UninstallString == nil || *app.UninstallString != strings.TrimSpace(longCommand) || app.QuietUninstallString != nil {
			t.Fatal("uninstall text altered")
		}
		if app.EstimatedSize != uint64(^uint32(0))*1024 || app.LastUsedRaw != 0 || app.UninstallerMissing {
			t.Fatal("unexpected enrichment")
		}
	}
}
