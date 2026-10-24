package platform

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCacheReleaseDarwinRefusesReusedIdentityBeforeQuit(t *testing.T) {
	pid := uint32(os.Getpid())
	id := ProcessUniqueID(pid)
	if id == nil {
		t.Fatal("native process identity unavailable")
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, force := range []bool{false, true} {
		if err := RequestCacheOwnerExit(CacheOccupant{PID: pid, Identity: *id ^ 1, Executable: exe}, force); err == nil {
			t.Fatal("reused native identity accepted")
		}
	}
}

func TestCacheReleaseDarwinProbeNeverFollowsRootLinks(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	cache := filepath.Join(root, "cache")
	if err := os.Mkdir(cache, 0700); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "link")
	if err := os.Symlink(cache, link); err != nil {
		t.Fatal(err)
	}
	if _, err := CacheReleaseOccupants(link); err == nil {
		t.Fatal("linked root accepted")
	}
}
