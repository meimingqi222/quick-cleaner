package core

import (
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func diskPruneCache(t *testing.T) (*DiskIndexCache, string) {
	t.Helper()
	root, e := filepath.EvalSymlinks(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	c, e := NewDiskIndexCache(root)
	if e != nil {
		t.Fatal(e)
	}
	return c, root
}
func diskPruneGroup(t *testing.T, c *DiskIndexCache, mount string, age time.Duration) []string {
	t.Helper()
	base, delta := c.Paths(VolumeFromMountPoint(mount))
	paths := []string{base, delta, diskIndexCoveragePath(base, 1), diskIndexCoveragePath(delta, 2)}
	for _, p := range paths {
		diskCacheWrite(t, p, []byte("stub"))
		when := time.Now().Add(-age)
		if e := os.Chtimes(p, when, when); e != nil {
			t.Fatal(e)
		}
	}
	return paths
}
func diskPruneAssert(t *testing.T, paths []string, exists bool) {
	t.Helper()
	for _, p := range paths {
		_, e := os.Lstat(p)
		if exists && e != nil {
			t.Fatalf("must retain %s: %v", p, e)
		}
		if !exists && !errors.Is(e, os.ErrNotExist) {
			t.Fatalf("must remove %s: %v", p, e)
		}
	}
}

func TestDiskIndexCachePruneOrphansPairsAndKeepsActive(t *testing.T) {
	c, _ := diskPruneCache(t)
	mount, e := filepath.EvalSymlinks(diskCacheVolumeDir(t))
	if e != nil {
		t.Fatal(e)
	}
	nested := filepath.Join(mount, "nested")
	if e = os.Mkdir(nested, 0700); e != nil {
		t.Fatal(e)
	}
	keep := VolumeFromMountPoint(mount)
	active := diskPruneGroup(t, c, mount, 90*24*time.Hour)
	if runtime.GOOS != "windows" {
		if err := os.Chmod(active[0], 0644); err != nil {
			t.Fatal(err)
		}
	}
	covered := diskPruneGroup(t, c, nested, 0)
	stale := diskPruneGroup(t, c, filepath.Join(diskCacheVolumeDir(t), "unmounted"), 31*24*time.Hour)
	unknown := diskPruneGroup(t, c, filepath.Join(mount, "not-mounted"), 0)
	fresh := diskPruneGroup(t, c, diskCacheVolumeDir(t), 29*24*time.Hour)
	future := diskPruneGroup(t, c, diskCacheVolumeDir(t), -24*time.Hour)
	n, e := c.PruneOrphans(keep)
	if e != nil || n != 2 {
		t.Fatalf("prune: %d %v", n, e)
	}
	diskPruneAssert(t, active, true)
	if runtime.GOOS != "windows" {
		info, err := os.Stat(active[0])
		if err != nil || info.Mode().Perm() != 0600 {
			t.Fatalf("active index permissions: %v %v", info, err)
		}
	}
	diskPruneAssert(t, covered, false)
	diskPruneAssert(t, stale, false)
	diskPruneAssert(t, unknown, true)
	diskPruneAssert(t, fresh, true)
	diskPruneAssert(t, future, true)
	n, e = c.PruneOrphans(keep)
	if e != nil || n != 0 {
		t.Fatalf("prune not idempotent: %d %v", n, e)
	}
}

func TestDiskIndexCachePruneStrictNamesAndNoLinks(t *testing.T) {
	c, root := diskPruneCache(t)
	keepMount, e := filepath.EvalSymlinks(diskCacheVolumeDir(t))
	if e != nil {
		t.Fatal(e)
	}
	keep := VolumeFromMountPoint(keepMount)
	old := time.Now().Add(-90 * 24 * time.Hour)
	invalidNames := []string{"scan-index-.bin", "scan-index-zz.bin", "scan-index-abc.bin", "scan-index-2F.bin", "scan-index-ff.bin", "scan-index-72656c6174697665.bin", "scan-index-2f612f2e2e2f62.bin", "scan-index-2f00.bin", "scan-index-2f.delta.bin", "unrelated.bin"}
	for _, name := range invalidNames {
		p := filepath.Join(root, name)
		diskCacheWrite(t, p, []byte("keep"))
		if e = os.Chtimes(p, old, old); e != nil {
			t.Fatal(e)
		}
	}
	outside := filepath.Join(t.TempDir(), "target")
	diskCacheWrite(t, outside, []byte("outside"))
	linkedBase, _ := c.Paths(VolumeFromMountPoint(filepath.Join(diskCacheVolumeDir(t), "linked-base")))
	if e = os.Symlink(outside, linkedBase); e != nil {
		t.Skipf("symlinks unavailable: %v", e)
	}
	paired := diskPruneGroup(t, c, filepath.Join(diskCacheVolumeDir(t), "paired"), 90*24*time.Hour)
	if e = os.Remove(paired[1]); e != nil {
		t.Fatal(e)
	}
	if e = os.Symlink(outside, paired[1]); e != nil {
		t.Fatal(e)
	}
	linkedMount := filepath.Join(keepMount, "link")
	target, e := filepath.EvalSymlinks(t.TempDir())
	if e != nil {
		t.Fatal(e)
	}
	if e = os.Symlink(target, linkedMount); e != nil {
		t.Fatal(e)
	}
	mountLinkGroup := diskPruneGroup(t, c, linkedMount, 0)
	n, e := c.PruneOrphans(keep)
	if e != nil || n != 0 {
		t.Fatalf("unsafe prune: %d %v", n, e)
	}
	for _, name := range invalidNames {
		diskPruneAssert(t, []string{filepath.Join(root, name)}, true)
	}
	diskPruneAssert(t, []string{outside, linkedBase}, true)
	diskPruneAssert(t, paired, true)
	diskPruneAssert(t, mountLinkGroup, true)
	if string(diskCacheRead(t, outside)) != "outside" {
		t.Fatal("followed link target")
	}
	// Redirecting the cache root itself or any ancestor is not cleanup authority.
	link := filepath.Join(t.TempDir(), "cache-link")
	if e = os.Symlink(root, link); e != nil {
		t.Fatal(e)
	}
	redirected, e := NewDiskIndexCache(link)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = redirected.PruneOrphans(keep); e == nil {
		t.Fatal("redirected root accepted")
	}
	if e = os.Mkdir(filepath.Join(root, "child"), 0700); e != nil {
		t.Fatal(e)
	}
	redirected, e = NewDiskIndexCache(filepath.Join(link, "child"))
	if e != nil {
		t.Fatal(e)
	}
	if _, e = redirected.PruneOrphans(keep); e == nil {
		t.Fatal("redirected root ancestor accepted")
	}
	if _, e = c.PruneOrphans(VolumeID{}); e == nil {
		t.Fatal("unknown active mount accepted")
	}
}

func TestDiskIndexCachePruneDifferentDeviceIsNotCovered(t *testing.T) {
	// /dev/shm is a disposable second-device fixture on Linux, not a user cache.
	// Native platforms without it still exercise unknown-device rejection above.
	other, e := os.MkdirTemp("/dev/shm", "quickcleaner-cache-prune-")
	if e != nil {
		t.Skip("no disposable second filesystem")
	}
	t.Cleanup(func() { os.RemoveAll(other) })
	if diskIndexSamePhysicalDevice(other, string(filepath.Separator)) {
		t.Skip("no distinct device")
	}
	c, _ := diskPruneCache(t)
	active := diskPruneGroup(t, c, string(filepath.Separator), 90*24*time.Hour)
	files := diskPruneGroup(t, c, other, 0)
	n, e := c.PruneOrphans(VolumeFromMountPoint(string(filepath.Separator)))
	if e != nil || n != 0 {
		t.Fatalf("cross-device subset pruned: %d %v", n, e)
	}
	diskPruneAssert(t, active, true)
	diskPruneAssert(t, files, true)
}

func TestDiskIndexCachePruneMountKeyValidation(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("POSIX mount-path fixture; the disk index cache is a macOS product path")
	}
	for _, mount := range []string{string(filepath.Separator), t.TempDir(), filepath.Join(t.TempDir(), "中文 mount")} {
		name := "scan-index-" + hex.EncodeToString([]byte(mount)) + ".bin"
		got, ok := diskIndexMountFromName(name)
		if !ok || got != mount {
			t.Fatalf("mount key %q: %q %v", name, got, ok)
		}
	}
}

func TestDiskIndexPruneOldTemporaryFiles(t *testing.T) {
	c, root := diskPruneCache(t)
	old := filepath.Join(root, "scan-index-"+hex.EncodeToString([]byte(root))+".bin.tmp")
	fresh := filepath.Join(root, ".scan-index-123.tmp")
	for _, p := range []string{old, fresh} {
		if err := os.WriteFile(p, []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	when := time.Now().Add(-31 * 24 * time.Hour)
	if err := os.Chtimes(old, when, when); err != nil {
		t.Fatal(err)
	}
	if _, err := c.PruneOrphans(VolumeFromMountPoint(root)); err != nil {
		t.Fatal(err)
	}
	diskPruneAssert(t, []string{old}, false)
	diskPruneAssert(t, []string{fresh}, true)
}
