package core

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestAppReviewedScriptFrozenBytesAndLiteralEnvironment(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("actual sh/bash execution is a POSIX fixture")
	}
	home := filepath.Join(t.TempDir(), "home ' $(ignored)")
	bundle := filepath.Join(home, "Library", "Input Methods", "Fixture.app")
	script := filepath.Join(bundle, "Contents", "Resources", "uninstall.sh")
	if err := os.MkdirAll(filepath.Dir(script), 0700); err != nil {
		t.Fatal(err)
	}
	body := "test \"$1\" = '--keep' && printf '%s' \"$2\" > \"$HOME/arguments\" && rm -rf -- \"$HOME/Library/Input Methods/Fixture.app\"\n"
	if err := os.WriteFile(script, []byte(body), 0600); err != nil {
		t.Fatal(err)
	}
	const bundleID = "fixture.inputmethod"
	if err := os.WriteFile(filepath.Join(bundle, "Contents", "Info.plist"), []byte(bundleID), 0600); err != nil {
		t.Fatal(err)
	}
	data := filepath.Join(home, "user-data")
	if err := os.WriteFile(data, []byte("keep user data"), 0600); err != nil {
		t.Fatal(err)
	}
	injection := filepath.Join(home, "startup.sh")
	if err := os.WriteFile(injection, []byte("touch \"$HOME/untrusted-startup\"\n"), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("BASH_ENV", injection)
	spec := ScriptUninstaller{BundleID: bundleID, Script: "Contents/Resources/uninstall.sh", Interpreter: "bash", SHA256: []string{fmt.Sprintf("%x", sha256.Sum256([]byte(body)))}, Arguments: []string{"--keep", "a ' $(touch injected) ; &"}, PreservesUserData: true}
	metadata := func(path string) (*string, *string) {
		bytes, err := os.ReadFile(path)
		if err != nil {
			return nil, nil
		}
		id := string(bytes)
		return &id, nil
	}
	err := runReviewedScriptIn(InstalledApp{InstallLocation: &bundle}, spec, home, []string{bundle}, metadata, func(verified string, spec ScriptUninstaller, home string, elevated bool) error {
		if elevated {
			t.Fatal("user-only installation must not request elevation")
		}
		// A changed on-disk script must not replace the approved bytes.
		if err := os.WriteFile(script, []byte("exit 73\n"), 0600); err != nil {
			t.Fatal(err)
		}
		return executeReviewedAppScript(verified, spec, home, elevated)
	})
	if err != nil {
		t.Fatal(err)
	}
	if bytes, err := os.ReadFile(data); err != nil || string(bytes) != "keep user data" {
		t.Fatalf("user data not preserved: %q, %v", bytes, err)
	}
	if bytes, err := os.ReadFile(filepath.Join(home, "arguments")); err != nil || string(bytes) != "a ' $(touch injected) ; &" {
		t.Fatalf("literal arguments changed: %q, %v", bytes, err)
	}
	if _, err := os.Lstat(filepath.Join(home, "untrusted-startup")); !os.IsNotExist(err) {
		t.Fatal("inherited shell startup environment executed")
	}
}

func TestAppReviewedScriptRejectsUnknownCopiesAndIncompleteRemoval(t *testing.T) {
	root := t.TempDir()
	home := filepath.Join(root, "home")
	bundle := filepath.Join(home, "Library", "Input Methods", "Fixture.app")
	systemCopy := filepath.Join(root, "system", "Library", "Input Methods", "Fixture.app")
	body := "exit 0\n"
	spec := ScriptUninstaller{BundleID: "fixture.inputmethod", Script: "Contents/uninstall.sh", Interpreter: "sh", SHA256: []string{fmt.Sprintf("%x", sha256.Sum256([]byte(body)))}}
	makeBundle := func(path, id string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Join(path, "Contents"), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "Contents", "Info.plist"), []byte(id), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "Contents", "uninstall.sh"), []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
	}
	makeBundle(bundle, spec.BundleID)
	makeBundle(systemCopy, "another.inputmethod")
	metadata := func(path string) (*string, *string) {
		bytes, err := os.ReadFile(path)
		if err != nil {
			return nil, nil
		}
		id := string(bytes)
		return &id, nil
	}
	app := InstalledApp{InstallLocation: &bundle}
	notExecuted := func(string, ScriptUninstaller, string, bool) error { t.Fatal("unverified script executed"); return nil }
	if err := runReviewedScriptIn(app, spec, home, []string{bundle, systemCopy}, metadata, notExecuted); err == nil {
		t.Fatal("foreign second copy was accepted")
	}
	makeBundle(systemCopy, spec.BundleID)
	unknown := spec
	unknown.SHA256 = []string{strings.Repeat("0", 64)}
	if err := runReviewedScriptIn(app, unknown, home, []string{bundle, systemCopy}, metadata, notExecuted); err == nil {
		t.Fatal("unreviewed script was accepted")
	}
	if err := runReviewedScriptIn(app, spec, home, []string{bundle, systemCopy}, metadata, func(_ string, _ ScriptUninstaller, _ string, elevated bool) error {
		if !elevated {
			t.Fatal("a system copy requires elevation")
		}
		return os.RemoveAll(bundle)
	}); err == nil {
		t.Fatal("one removed copy must not count as complete uninstall")
	}
	if _, err := os.Stat(systemCopy); err != nil {
		t.Fatal("failed completion must not fall back to deleting remaining copy")
	}
	if runtime.GOOS != "windows" {
		makeBundle(bundle, spec.BundleID)
		script := filepath.Join(bundle, "Contents", "uninstall.sh")
		external := filepath.Join(root, "external.sh")
		if err := os.Rename(script, external); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(external, script); err != nil {
			t.Fatal(err)
		}
		if err := runReviewedScriptIn(app, spec, home, []string{bundle}, metadata, notExecuted); err == nil {
			t.Fatal("redirected reviewed script was accepted")
		}
		if err := os.Remove(script); err != nil {
			t.Fatal(err)
		}
		for _, body := range []string{"exit 0\n", "exit 3\n"} {
			if err := os.WriteFile(script, []byte(body), 0600); err != nil {
				t.Fatal(err)
			}
			spec.SHA256 = []string{fmt.Sprintf("%x", sha256.Sum256([]byte(body)))}
			if err := runReviewedScriptIn(app, spec, home, []string{bundle}, metadata, executeReviewedAppScript); err == nil {
				t.Fatalf("script %q falsely reported an intact bundle removed", body)
			}
			if _, err := os.Stat(bundle); err != nil {
				t.Fatal("failed/no-op script fell back to deletion")
			}
		}
	}
}
