//go:build darwin || linux

package platform

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestPublicPOSIXEnvironment(t *testing.T) {
	for _, key := range []string{"LC_ALL", "LC_MESSAGES", "LANG"} {
		t.Setenv(key, "")
	}
	if got := SystemLanguageTag(); got != "" {
		t.Fatal(got)
	}
	for _, pair := range [][2]string{{"LANG", "en_US.UTF-8"}, {"LC_MESSAGES", "zh_TW.UTF-8"}, {"LC_ALL", "C"}} {
		t.Setenv(pair[0], pair[1])
		if got := SystemLanguageTag(); got != pair[1] {
			t.Fatalf("%s: %q", pair[0], got)
		}
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("TMPDIR", filepath.Join(home, "not-created"))
	if got, ok := UserHome(); !ok || got != home {
		t.Fatalf("home %q %v", got, ok)
	}
	if got, ok := UserTempDir(); !ok || got != os.TempDir() {
		t.Fatalf("temp %q %v", got, ok)
	}
	if runtime.GOOS == "darwin" {
		if got, ok := UserCacheDir(); !ok || got != filepath.Join(home, "Library", "Caches") {
			t.Fatalf("cache %q %v", got, ok)
		}
		if got, ok := UserDataDir(); !ok || got != filepath.Join(home, "Library", "Application Support") {
			t.Fatalf("data %q %v", got, ok)
		}
	}
}
