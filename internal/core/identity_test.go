package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func identityWrite(t *testing.T, path, content string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

func identityCapture(t *testing.T, path string) TargetIdentity {
	t.Helper()
	id, ok := CaptureIdentity(path)
	if !ok {
		t.Fatalf("capture %s failed", path)
	}
	return id
}

func TestIdentityUnchangedReplacementMissing(t *testing.T) {
	path := filepath.Join(t.TempDir(), "target")
	identityWrite(t, path, "old")
	id := identityCapture(t, path)
	if !id.Recheck(path) {
		t.Fatal("unchanged object rejected")
	}
	// Keep the original object alive so inode/file-index reuse cannot make
	// this test nondeterministic.
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, path, "replacement")
	if id.Recheck(path) {
		t.Fatal("replacement accepted")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if id.Recheck(path) {
		t.Fatal("missing object accepted")
	}
	if _, ok := CaptureIdentity(path); ok {
		t.Fatal("captured missing object")
	}
}

func TestIdentityInPlaceWrite(t *testing.T) {
	path := filepath.Join(t.TempDir(), "target")
	identityWrite(t, path, "old")
	id := identityCapture(t, path)
	identityWrite(t, path, "longer in-place contents")
	if got, want := id.Recheck(path), runtime.GOOS != "windows"; got != want {
		t.Fatalf("in-place verdict %v, want %v", got, want)
	}
}

func TestIdentityAncestorSymlinkSwap(t *testing.T) {
	base := t.TempDir()
	parent, other := filepath.Join(base, "parent"), filepath.Join(base, "other")
	for _, dir := range []string{parent, other} {
		if err := os.Mkdir(dir, 0700); err != nil {
			t.Fatal(err)
		}
		identityWrite(t, filepath.Join(dir, "target"), "same")
	}
	path := filepath.Join(parent, "target")
	id := identityCapture(t, path)
	if err := os.Rename(parent, parent+".old"); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(other, parent); err != nil {
		if runtime.GOOS == "windows" {
			t.Skipf("symlink privilege unavailable: %v", err)
		}
		t.Fatal(err)
	}
	if id.Recheck(path) {
		t.Fatal("ancestor swap accepted")
	}
}

func TestIdentityUnixDirectoryChurn(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix dev/inode semantics")
	}
	dir := t.TempDir()
	id := identityCapture(t, dir)
	child := filepath.Join(dir, "new-child")
	identityWrite(t, child, "child")
	if !id.Recheck(dir) {
		t.Fatal("directory addition changed identity")
	}
	if err := os.Remove(child); err != nil {
		t.Fatal(err)
	}
	if !id.Recheck(dir) {
		t.Fatal("directory removal changed identity")
	}
}

type identityTimeInfo struct {
	os.FileInfo
	modified time.Time
}

func (i identityTimeInfo) ModTime() time.Time { return i.modified }

func TestIdentityMetadataTimes(t *testing.T) {
	path := filepath.Join(t.TempDir(), "target")
	identityWrite(t, path, "x")
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := IdentityFromMetadata(nil); ok {
		t.Fatal("nil metadata accepted")
	}
	if _, ok := IdentityFromMetadata(identityTimeInfo{FileInfo: info}); ok {
		t.Fatal("unavailable mtime accepted")
	}
	id, ok := IdentityFromMetadata(identityTimeInfo{info, time.Unix(-2, 500000000)})
	if !ok || id.mtime != -1 {
		t.Fatalf("pre-epoch truncation: %+v %v", id, ok)
	}
}
