package core

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestBrokenLoginItemRequiresMissingAbsoluteExecutable(t *testing.T) {
	root := t.TempDir()
	home := filepath.Join(root, "home")
	agents := filepath.Join(home, "Library", "LaunchAgents")
	if err := os.MkdirAll(agents, 0o700); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(root, "gone")
	present := filepath.Join(root, "present")
	if err := os.WriteFile(present, []byte("x"), 0o700); err != nil {
		t.Fatal(err)
	}
	old := time.Now().Add(-48 * time.Hour)
	writePlist := func(name string) string {
		t.Helper()
		path := filepath.Join(agents, name)
		if err := os.WriteFile(path, []byte("plist"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Chtimes(path, old, old); err != nil {
			t.Fatal(err)
		}
		return path
	}
	gone := writePlist("gone.plist")
	alive := writePlist("alive.plist")
	relative := writePlist("relative.plist")
	empty := writePlist("empty.plist")
	malformed := writePlist("bad.plist")
	fresh := filepath.Join(agents, "fresh.plist")
	if err := os.WriteFile(fresh, []byte("plist"), 0o600); err != nil {
		t.Fatal(err)
	}
	plists := map[string][]byte{
		gone:      []byte(`{"Program":"` + missing + `"}`),
		alive:     []byte(`{"ProgramArguments":["` + present + `","--flag"]}`),
		relative:  []byte(`{"Program":"helper"}`),
		empty:     []byte(`{"Label":"nothing"}`),
		malformed: []byte(`not json`),
		fresh:     []byte(`{"Program":"` + missing + `"}`),
	}
	read := func(path string) ([]byte, error) { return plists[path], nil }

	if !launchAgentExecutableMissing(gone, read) {
		t.Fatal("missing absolute executable was not broken")
	}
	for _, path := range []string{alive, relative, malformed} {
		if launchAgentExecutableMissing(path, read) {
			t.Fatal("non-evidence treated as broken", path)
		}
	}
	if !launchAgentExecutableMissing(empty, read) {
		t.Fatal("plist with no executable entry is itself broken")
	}
	rules, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	original := launchAgentReader
	launchAgentReader = read
	t.Cleanup(func() { launchAgentReader = original })
	targets := brokenLoginItemTargets(rules, home)
	if len(targets) != 2 {
		t.Fatalf("expected the missing and empty plists only, got %+v", targets)
	}
	for _, target := range targets {
		if target.Category != BrokenLoginItems || target.Operation.Kind != OpFile || !target.Recommended {
			t.Fatalf("target not a recommended file removal: %+v", target)
		}
		if target.Path == fresh || target.Path == alive || target.Path == relative || target.Path == malformed {
			t.Fatal("excluded plist became a target", target.Path)
		}
	}
}
