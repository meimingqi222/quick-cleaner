package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestAppBundleDeclaredUninstallerNeverFallsBack(t *testing.T) {
	bundle := filepath.Join(t.TempDir(), "Zoom.app")
	generic := filepath.Join(bundle, "Contents", "Resources", "Uninstall Other.app")
	if err := os.MkdirAll(generic, 0700); err != nil {
		t.Fatal(err)
	}
	if path := MacAppUninstaller(bundle, "us.zoom.xos"); path != nil {
		t.Fatalf("missing declared Zoom route must not use a guessed uninstaller: %s", *path)
	}
	declared := filepath.Join(bundle, "Contents", "Frameworks", "ZoomUninstaller.app")
	if err := os.MkdirAll(declared, 0700); err != nil {
		t.Fatal(err)
	}
	if path := MacAppUninstaller(bundle, "us.zoom.xos"); path == nil || *path != declared {
		t.Fatalf("declared official route unavailable: %v", path)
	}
	if runtime.GOOS != "windows" {
		if err := os.Remove(declared); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(generic, declared); err != nil {
			t.Fatal(err)
		}
		if path := MacAppUninstaller(bundle, "us.zoom.xos"); path != nil {
			t.Fatalf("redirected declared route must not fall back: %s", *path)
		}
	}
	if path := MacAppUninstaller(bundle, "unrelated.bundle"); path == nil || *path != generic {
		t.Fatalf("generic resource uninstaller changed: %v", path)
	}
}
