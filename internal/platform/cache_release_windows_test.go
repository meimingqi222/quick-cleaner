package platform

import (
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/windows"
)

func TestCacheReleaseWindowsExactCreationTimeRefusesSameSecondReuse(t *testing.T) {
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, uint32(os.Getpid()))
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(h)
	var created, exited, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &created, &exited, &kernel, &user); err != nil {
		t.Fatal(err)
	}
	owner := CacheOccupant{PID: uint32(os.Getpid()), Identity: cacheFiletime(created)}
	if err := cacheVerifyHandle(h, owner, false); err != nil {
		t.Fatal(err)
	}
	// One 100ns tick differs while rounded Unix seconds remain identical.
	owner.Identity ^= 1
	if err := cacheVerifyHandle(h, owner, false); err == nil {
		t.Fatal("rounded process identity accepted")
	}
	if err := RequestCacheOwnerExit(owner, true); err == nil {
		t.Fatal("cleaner/reused process accepted")
	}
}

func TestCacheReleaseWindowsProbeNeverFollowsLinks(t *testing.T) {
	root := t.TempDir()
	cache := filepath.Join(root, "cache")
	if err := os.Mkdir(cache, 0700); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(root, "sentinel")
	if err := os.WriteFile(target, []byte("retain"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, filepath.Join(cache, "linked")); err != nil {
		t.Skipf("symlink privilege unavailable: %v", err)
	}
	if _, err := CacheReleaseOccupants(cache); err == nil {
		t.Fatal("linked tree reported known vacancy")
	}
	if _, err := os.Stat(target); err != nil {
		t.Fatal(err)
	}
}
