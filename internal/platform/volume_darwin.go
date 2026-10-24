package platform

import (
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/unix"
)

func ListVolumes() []VolumeID {
	var mounts []volumeMount
	var root unix.Statfs_t
	if unix.Statfs("/", &root) == nil {
		mounts = append(mounts, volumeMount{
			mount: "/", device: unix.ByteSliceToString(root.Mntfromname[:]),
			total: root.Blocks * uint64(root.Bsize),
		})
	}
	entries, _ := os.ReadDir("/Volumes")
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".") || strings.Contains(entry.Name(), ".hidden") {
			continue
		}
		path := filepath.Join("/Volumes", entry.Name())
		// Follow the root-volume symlink just as Rust Path::is_dir does.
		info, err := os.Stat(path)
		if err != nil || !info.IsDir() {
			continue
		}
		var stat unix.Statfs_t
		if unix.Statfs(path, &stat) != nil {
			continue
		}
		mounts = append(mounts, volumeMount{
			mount: path, device: unix.ByteSliceToString(stat.Mntfromname[:]),
			label: entry.Name(), filesystem: unix.ByteSliceToString(stat.Fstypename[:]),
			total: stat.Blocks * uint64(stat.Bsize),
		})
	}
	return groupMacVolumes(mounts)
}

// APFS reports container-wide space; Bavail excludes reserved/purgeable space.
func GetVolumeSpace(vol VolumeID) (total, free uint64, ok bool) {
	var stat unix.Statfs_t
	if unix.Statfs(vol.MountPoint(), &stat) != nil {
		return 0, 0, false
	}
	return stat.Blocks * uint64(stat.Bsize), stat.Bavail * uint64(stat.Bsize), true
}
