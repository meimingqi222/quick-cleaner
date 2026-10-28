package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func devFixtureRun(answers map[string]ProcRun) DevToolRunner {
	return func(program string, args []string, _ time.Duration) (ProcRun, bool) {
		r, ok := answers[program+" "+strings.Join(args, " ")]
		return r, ok
	}
}
func devFixtureJSON(value any) string { b, _ := json.Marshal(value); return string(b) }
func devFixtureWrite(t *testing.T, path, text string) {
	t.Helper()
	if e := os.MkdirAll(filepath.Dir(path), 0755); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(path, []byte(text), 0644); e != nil {
		t.Fatal(e)
	}
}

func TestDevInventoryNpmContract(t *testing.T) {
	prefix := t.TempDir()
	for _, tc := range []struct {
		name, text string
		ok         bool
		want       InventoryError
		count      int
	}{
		{"nonzero usable", `{"dependencies":{"typescript":{"version":"5.4.2","dependencies":{"nested":{}}}}}`, false, "", 1},
		{"empty authoritative", `{"dependencies":{}}`, true, "", 0},
		{"malformed", `not json`, true, InventoryUnreadable, 0},
		{"missing dependencies", `{}`, true, InventoryUnreadable, 0},
		{"null dependencies", `{"dependencies":null}`, true, InventoryUnreadable, 0},
		{"blank", ` `, true, InventoryUnavailable, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			run := devFixtureRun(map[string]ProcRun{"npm prefix --global": {Stdout: []byte(prefix + "\n"), OK: true}, "npm ls --global --depth=0 --json": {Stdout: []byte(tc.text), OK: tc.ok}})
			packages, e := InventoryNpm(run)
			if e != tc.want || len(packages) != tc.count {
				t.Fatalf("got %+v, %q", packages, e)
			}
			if len(packages) > 0 {
				if packages[0].Path != filepath.Join(prefix, "node_modules", "typescript") || packages[0].Version == nil || *packages[0].Version != "5.4.2" {
					t.Fatal(packages)
				}
			}
		})
	}
}

func TestDevInventoryFailureAndOptionalFacts(t *testing.T) {
	for _, tc := range []struct {
		text string
		ok   bool
		want InventoryError
	}{{"", true, InventoryUnavailable}, {"bad", true, InventoryUnreadable}, {`{"envs":[]}`, false, InventoryUnavailable}, {`{"envs":null}`, true, InventoryUnreadable}, {`{"envs":[]}`, true, ""}} {
		_, e := InventoryConda(devFixtureRun(map[string]ProcRun{"conda info --json": {Stdout: []byte(tc.text), OK: tc.ok}}), "conda")
		if e != tc.want {
			t.Fatalf("%q: %q", tc.text, e)
		}
	}
	if _, e := InventoryConda(devFixtureRun(nil), "conda"); e != InventoryUnavailable {
		t.Fatal(e)
	}
	if _, e := InventoryRoot(devFixtureRun(map[string]ProcRun{"uv tool dir": {Stdout: []byte("relative"), OK: true}}), "uv"); e != InventoryUnreadable {
		t.Fatal(e)
	}
	tools, e := InventoryPipx(devFixtureRun(map[string]ProcRun{"pipx list --json": {Stdout: []byte(`{"venvs":{"black":{},"ruff":{"metadata":{"environment":"/tools/ruff"}}}}`), OK: true}}))
	if e != "" || len(tools) != 2 || tools[0].Venv != nil || tools[1].Venv == nil || *tools[1].Venv != "/tools/ruff" {
		t.Fatalf("%+v %q", tools, e)
	}
}
