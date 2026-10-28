package platform

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"syscall"
	"testing"
)

func TestFullDiskAccessNeedsReadableProbeAndPreservesPriority(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Full Disk Access permission-bit fixture; macOS product path")
	}
	for _, mode := range []string{"missing", "database", "denied-database", "mail", "safari", "unreadable-first"} {
		t.Run(mode, func(t *testing.T) {
			home := t.TempDir()
			library := filepath.Join(home, "Library")
			if err := os.MkdirAll(library, 0755); err != nil {
				t.Fatal(err)
			}
			switch mode {
			case "database", "denied-database":
				parent := filepath.Join(library, "Application Support", "com.apple.TCC")
				if err := os.MkdirAll(parent, 0755); err != nil {
					t.Fatal(err)
				}
				probe := filepath.Join(parent, "TCC.db")
				if err := os.WriteFile(probe, []byte("database"), 0600); err != nil {
					t.Fatal(err)
				}
				if mode == "denied-database" {
					if err := os.Chmod(probe, 0); err != nil {
						t.Fatal(err)
					}
					if file, err := os.Open(probe); err == nil {
						file.Close()
						t.Skip("this account/filesystem can read mode-000 files")
					}
					if _, err := os.Stat(probe); err != nil {
						t.Fatal("probe metadata must remain visible", err)
					}
				}
			case "mail", "unreadable-first":
				if err := os.Mkdir(filepath.Join(library, "Mail"), 0755); err != nil {
					t.Fatal(err)
				}
				if mode == "unreadable-first" {
					if err := os.WriteFile(filepath.Join(library, "Application Support"), nil, 0600); err != nil {
						t.Fatal(err)
					}
				}
			case "safari":
				if err := os.Mkdir(filepath.Join(library, "Safari"), 0755); err != nil {
					t.Fatal(err)
				}
			}
			want := mode != "missing" && mode != "unreadable-first" && mode != "denied-database"
			if got := ProbeFullDiskAccess(home); got != want {
				t.Fatalf("%s: got %v, want %v", mode, got, want)
			}
		})
	}
	if !IsTCCDenied(&os.PathError{Op: "open", Path: "probe", Err: syscall.EPERM}) || IsTCCDenied(fmt.Errorf("not a TCC denial: %w", syscall.EACCES)) {
		t.Fatal("TCC denial confused with ordinary access denial")
	}
}

func TestAppBundleOnlyAtExpectedExecutableDepth(t *testing.T) {
	for _, tc := range []struct {
		relative, bundle string
		ok               bool
	}{
		{"QuickCleaner.app/Contents/MacOS/quick-cleaner", "QuickCleaner.app", true},
		{"QuickCleaner.APP/Contents/MacOS/quick-cleaner", "", false},
		{"QuickCleaner.app/Contents/Helpers/nested/tool", "", false},
		{"target/debug/quick-cleaner", "", false},
	} {
		root := t.TempDir()
		bundle, ok := AppBundleForExecutable(filepath.Join(root, filepath.FromSlash(tc.relative)))
		if ok != tc.ok || ok && bundle != filepath.Join(root, tc.bundle) {
			t.Fatal(tc, bundle, ok)
		}
	}
}
