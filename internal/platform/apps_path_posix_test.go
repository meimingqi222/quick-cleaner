//go:build darwin || linux

package platform

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCanonicalAppPathPreservesEntriesAndResolvesAliases(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "MixedCase.app")
	if err := os.WriteFile(path, []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	canonical, ok := CanonicalAppPath(path)
	if !ok {
		t.Fatal("native realpath unavailable")
	}
	alias := filepath.Join(root, "alias")
	if err := os.Symlink(path, alias); err != nil {
		t.Fatal(err)
	}
	if actual, ok := CanonicalAppPath(alias); !ok || actual != canonical {
		t.Fatalf("canonical=%q alias=%q known=%v", canonical, actual, ok)
	}
	hardlink := filepath.Join(root, "other-entry")
	if err := os.Link(path, hardlink); err != nil {
		t.Fatal(err)
	}
	if actual, ok := CanonicalAppPath(hardlink); !ok || actual == canonical {
		t.Fatalf("hardlink directory entries must remain distinct: %q known=%v", actual, ok)
	}
	for _, invalid := range []string{"", "\x00", filepath.Join(root, "missing")} {
		if actual, ok := CanonicalAppPath(invalid); ok {
			t.Fatalf("invalid %q canonicalized to %q", invalid, actual)
		}
	}
	t.Run("case-insensitive-volume", func(t *testing.T) {
		alias := filepath.Join(root, "mixedcase.app")
		if _, err := os.Stat(alias); os.IsNotExist(err) {
			t.Skip("fixture volume is case-sensitive")
		}
		if actual, ok := CanonicalAppPath(alias); !ok || actual != canonical {
			t.Fatalf("same entry has two canonical spellings: %q and %q known=%v", canonical, actual, ok)
		}
	})
}
