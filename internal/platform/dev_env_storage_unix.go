//go:build linux || darwin

package platform

import (
	"os"
	"syscall"
)

// DevEnvStorageFileInfo returns link count and native file identity, without
// the deletion-specific ancestor checks of ObjectID on Windows.
func DevEnvStorageFileInfo(path string) (uint32, [2]uint64, bool) {
	info, err := os.Lstat(path)
	if err != nil {
		return 0, [2]uint64{}, false
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat == nil {
		return 0, [2]uint64{}, false
	}
	return uint32(stat.Nlink), [2]uint64{uint64(stat.Dev), stat.Ino}, true
}

// DevEnvStorageIsReparse includes all entries that storage traversal must skip.
func DevEnvStorageIsReparse(info os.FileInfo) bool {
	return info.Mode()&os.ModeSymlink != 0
}
