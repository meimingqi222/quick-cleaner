//go:build darwin || linux

package platform

import (
	"testing"

	"golang.org/x/sys/unix"
)

func TestVolumeNativeRootSpaceAndListing(t *testing.T) {
	var stat unix.Statfs_t
	if err := unix.Statfs("/", &stat); err != nil {
		t.Fatal(err)
	}
	total, free, ok := GetVolumeSpace(VolumeFromMountPoint("/"))
	if !ok || total == 0 || free > total || total != stat.Blocks*uint64(stat.Bsize) {
		t.Fatalf("root space = %d, %d, %v", total, free, ok)
	}
	for _, vol := range ListVolumes() {
		if vol.MountPoint() == "/" {
			return
		}
	}
	t.Fatal("ListVolumes omitted root")
}

func TestVolumeNativeInvalidPath(t *testing.T) {
	for _, path := range []string{"/\x00invalid", "/dev/null/not-a-directory"} {
		if total, free, ok := GetVolumeSpace(VolumeFromMountPoint(path)); ok || total != 0 || free != 0 {
			t.Errorf("invalid path %q: %d, %d, %v", path, total, free, ok)
		}
	}
}
