package platform

import "golang.org/x/sys/unix"

// Linux is not a source-product platform. This read-only fallback exposes only
// the root filesystem, not a Linux equivalent of macOS physical-container discovery.
func ListVolumes() []VolumeID {
	root := VolumeFromMountPoint("/")
	if _, _, ok := GetVolumeSpace(root); !ok {
		return nil
	}
	return []VolumeID{root}
}

func GetVolumeSpace(vol VolumeID) (total, free uint64, ok bool) {
	var stat unix.Statfs_t
	if unix.Statfs(vol.MountPoint(), &stat) != nil {
		return 0, 0, false
	}
	return stat.Blocks * uint64(stat.Bsize), stat.Bavail * uint64(stat.Bsize), true
}
