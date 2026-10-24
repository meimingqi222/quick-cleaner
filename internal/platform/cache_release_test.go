package platform

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCacheReleaseLsofAllOwnersAndPathBoundaries(t *testing.T) {
	root := filepath.Join(t.TempDir(), "cache")
	data := []byte("p40\x00\nn" + filepath.Join(root, "a") + "\x00\nn" + filepath.Join(root, "b") + "\x00\np41\x00\nn" + filepath.Join(root, "c") + "\x00\np42\x00\nn" + root + "-other/x\x00\n")
	owners, err := parseCacheLsof(data, root)
	if err != nil || len(owners) != 2 || owners[40] != filepath.Join(root, "a") || owners[41] != filepath.Join(root, "c") {
		t.Fatalf("owners=%v err=%v", owners, err)
	}
	for _, data := range [][]byte{[]byte("p40"), []byte("p40\x00\n"), []byte("n" + root + "/a\x00\n"), []byte("p0\x00\nn" + root + "/a\x00\n"), []byte("p40\x00\nxunknown\x00\n"), []byte("p40\x00\nnrelative\x00\n"), []byte("p40\x00\np41\x00\nn" + root + "/a\x00\n")} {
		if _, err := parseCacheLsof(data, root); err == nil {
			t.Fatalf("partial/unknown output accepted: %q", data)
		}
	}
	if owners, err := parseCacheLsof(nil, root); err != nil || len(owners) != 0 {
		t.Fatal("known empty", err)
	}
}

func TestCacheReleaseNeverTargetsCleanerItself(t *testing.T) {
	if _, _, err := cacheProcessSnapshot(uint32(1)); err == nil {
		t.Fatal("system PID accepted")
	}
	if _, _, err := cacheProcessSnapshot(uint32(os.Getpid())); err == nil {
		t.Fatal("cleaner PID accepted")
	}
}
