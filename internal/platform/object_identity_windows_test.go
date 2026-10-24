package platform

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestObjectIDNative(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "file")
	if err := os.WriteFile(path, []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, p := range []string{dir, path} {
		id, ok := ObjectID(p)
		again, okAgain := ObjectID(p)
		if !ok || !okAgain || id != again {
			t.Fatalf("unstable identity for %s", p)
		}
	}
	for _, p := range []string{"", filepath.Join(dir, "missing"), filepath.Join(dir, strings.Repeat(`a\`, 129), "file")} {
		if _, ok := ObjectID(p); ok {
			t.Fatalf("invalid/deep path accepted: %q", p)
		}
	}
}

func TestObjectIDRejectsNativeReparsePoints(t *testing.T) {
	dir := t.TempDir()
	real, link := filepath.Join(dir, "real"), filepath.Join(dir, "link")
	if err := os.Mkdir(real, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(real, "file"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(real, link); err != nil {
		t.Skipf("symlink privilege unavailable: %v", err)
	}
	for _, path := range []string{link, filepath.Join(link, "file")} {
		if _, ok := ObjectID(path); ok {
			t.Fatalf("reparse path accepted: %s", path)
		}
	}
}

func TestObjectIDRejectsNativeJunction(t *testing.T) {
	dir := t.TempDir()
	real, link := filepath.Join(dir, "real"), filepath.Join(dir, "junction")
	if err := os.Mkdir(real, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(real, "file"), []byte("x"), 0600); err != nil {
		t.Fatal(err)
	}
	if output, err := exec.Command("cmd.exe", "/c", "mklink", "/J", link, real).CombinedOutput(); err != nil {
		t.Fatalf("create native junction: %v: %s", err, output)
	}
	for _, path := range []string{link, filepath.Join(link, "file")} {
		if _, ok := ObjectID(path); ok {
			t.Fatalf("junction path accepted: %s", path)
		}
	}
}
