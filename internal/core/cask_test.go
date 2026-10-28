package core

import (
	"encoding/json"
	"testing"
)

func caskTestJSON(t *testing.T, s string) map[string]any {
	t.Helper()
	var v map[string]any
	if err := json.Unmarshal([]byte(s), &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestInstalledCaskScopeExactPersistedTarget(t *testing.T) {
	receipt := caskTestJSON(t, `{"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["subdir/Original.app",{"target":"Renamed.app"}]},{"uninstall":[{"quit":["com.example.main"]}]},{"zap":[{"trash":"~/Documents/*"}]}]}`)
	config := caskTestJSON(t, `{"default":{"appdir":"/Applications"},"env":{"appdir":"/Other"},"explicit":{"appdir":"~/Applications"}}`)
	scope, err := ParseInstalledCaskScope(receipt, nil, config, "/test-home", "com.example.main")
	if err != nil || !scope.Authorizes("/test-home/Applications/Renamed.app") {
		t.Fatalf("%+v %v", scope, err)
	}
	for _, wrong := range []string{"/Applications/Renamed.app", "/test-home/Applications/Original.app", "/other/Renamed.app"} {
		if scope.Authorizes(wrong) {
			t.Fatal(wrong)
		}
	}
	definition := caskTestJSON(t, `{"artifacts":[{"app":["Original.app",{"target":"/Applications/Absolute.app"}],"target":"/Applications/Absolute.app"}]}`)
	scope, err = ParseInstalledCaskScope(receipt, definition, config, "/test-home", "com.example.main")
	if err != nil || !scope.Authorizes("/Applications/Absolute.app") {
		t.Fatalf("%+v %v", scope, err)
	}
	definition["artifacts"] = []any{}
	scope, err = ParseInstalledCaskScope(receipt, definition, config, "/test-home", "com.example.main")
	if err != nil || len(scope.Apps) != 0 || scope.AppOnly {
		t.Fatalf("empty definition recovered receipt: %+v %v", scope, err)
	}
}

func TestInstalledCaskScopeRefusesUnknownAndIncidentalRemoval(t *testing.T) {
	config := caskTestJSON(t, `{"default":{"appdir":"/Applications"}}`)
	for _, extra := range []string{
		`{"app":["Companion.app"]}`, `{"binary":["tool"]}`, `{"uninstall":[{"delete":"~/Documents"}]}`,
		`{"uninstall":[{"script":"remove.sh"}]}`, `{"uninstall":[{"pkgutil":"com.example.pkg"}]}`,
		`{"uninstall":[{"launchctl":"com.example.service"}]}`, `{"uninstall":[{"quit":"com.example.other"}]}`,
		`{"uninstall":[]}`, `{"uninstall":[{"quit":[]}]}`, `{"future":true}`,
	} {
		t.Run(extra, func(t *testing.T) {
			receipt := caskTestJSON(t, `{"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Main.app"]},`+extra+`]}`)
			scope, err := ParseInstalledCaskScope(receipt, nil, config, "/test-home", "com.example.main")
			if err != nil || scope.Authorizes("/Applications/Main.app") || len(scope.Apps) == 0 {
				t.Fatalf("%+v %v", scope, err)
			}
		})
	}
	for _, definition := range []string{`{"variations":{"arm64_sonoma":{}}}`, `{"variations":null}`, `{"artifacts":[{"app":["Main.app",{"target":"Main.app","unknown":true}]}]}`} {
		receipt := caskTestJSON(t, `{"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Main.app"]}]}`)
		scope, err := ParseInstalledCaskScope(receipt, caskTestJSON(t, definition), config, "/test-home", "com.example.main")
		if err != nil || scope.Authorizes("/Applications/Main.app") {
			t.Fatalf("%s: %+v %v", definition, scope, err)
		}
	}
	for _, flag := range []any{nil, true, "false"} {
		receipt := caskTestJSON(t, `{"uninstall_artifacts":[{"app":["Main.app"]}]}`)
		receipt["uninstall_flight_blocks"] = flag
		scope, err := ParseInstalledCaskScope(receipt, nil, config, "/test-home", "com.example.main")
		if err != nil || scope.AppOnly {
			t.Fatalf("%+v %v", scope, err)
		}
	}
}

func TestInstalledCaskScopeRejectsUnconfinedOrInvalidEvidence(t *testing.T) {
	config := caskTestJSON(t, `{"default":{"appdir":"/Applications"}}`)
	for _, artifact := range []string{
		`{"app":["../Other.app"]}`, `{"app":["/Absolute.app"]}`, `{"app":["Original.app",{"target":"../Outside.app"}]}`,
		`{"app":["Original.app",{"target":"/Applications/../Outside.app"}]}`, `{"app":["Original.app",{"target":"bad\\Target.app"}]}`,
		`{"app":["Original.app"],"target":"/Different.app"}`, `{"app":["Original.app",{}]}`, `{"app":[]}`, `{"app":["not-an-app"]}`, `null`,
	} {
		receipt := caskTestJSON(t, `{"uninstall_flight_blocks":false,"uninstall_artifacts":[`+artifact+`]}`)
		if _, err := ParseInstalledCaskScope(receipt, nil, config, "/test-home", "com.example.main"); err == nil {
			t.Fatal(artifact)
		}
	}
	for _, cfg := range []string{`{}`, `{"explicit":{"appdir":null},"default":{"appdir":"/Applications"}}`, `{"default":{"appdir":"relative"}}`, `{"default":{"appdir":"/Apps/../Applications"}}`} {
		receipt := caskTestJSON(t, `{"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Main.app"]}]}`)
		if _, err := ParseInstalledCaskScope(receipt, nil, caskTestJSON(t, cfg), "/test-home", "com.example.main"); err == nil {
			t.Fatal(cfg)
		}
	}
	if _, err := ParseInstalledCaskScope(nil, nil, config, "/test-home", "id"); err == nil {
		t.Fatal("missing scope accepted")
	}
}
