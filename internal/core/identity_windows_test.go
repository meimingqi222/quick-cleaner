package core

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestIdentityWindowsSameSecondSameLengthReplacement(t *testing.T) {
	path := filepath.Join(t.TempDir(), "target")
	identityWrite(t, path, "old")
	stamp := time.Unix(1700000000, 0)
	if err := os.Chtimes(path, stamp, stamp); err != nil {
		t.Fatal(err)
	}
	strong := identityCapture(t, path)
	if !strong.hasStable {
		t.Fatal("native file did not yield stable identity")
	}
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}
	weak, ok := IdentityFromMetadata(info)
	if !ok || weak.hasStable {
		t.Fatal("metadata capture must remain weak")
	}
	if err := os.Rename(path, path+".old"); err != nil {
		t.Fatal(err)
	}
	identityWrite(t, path, "new")
	if err := os.Chtimes(path, stamp, stamp); err != nil {
		t.Fatal(err)
	}
	if strong.Recheck(path) {
		t.Fatal("stable identity accepted replacement")
	}
	if !weak.Recheck(path) {
		t.Fatal("weak fallback behavior changed")
	}
}

func TestIdentityWindowsReparseFallback(t *testing.T) {
	base := t.TempDir()
	real, link := filepath.Join(base, "real"), filepath.Join(base, "link")
	identityWrite(t, real, "data")
	if err := os.Symlink(real, link); err != nil {
		t.Skipf("symlink privilege unavailable: %v", err)
	}
	id := identityCapture(t, link)
	if id.hasStable || !id.Recheck(link) {
		t.Fatal("reparse capture must retain weak fallback")
	}
}
