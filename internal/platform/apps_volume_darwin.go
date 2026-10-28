package platform

import (
	"os"
	"path/filepath"
	"runtime"
	"syscall"
)

// macOnDataVolume reports whether path lives on the same volume as the system
// and user data (that is, this machine's own disk) rather than on a mounted
// image, external disk or network share.
//
// The probe compares the volume device of the target with the ones of the boot
// data volume. macOS boots from a read-only system volume and mounts user data
// separately, so both are accepted. The second result is false whenever the
// identity cannot be established; callers treat that as "still installed".
func macOnDataVolume(path string) (same bool, known bool) {
	if runtime.GOOS != "darwin" {
		return false, false
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		resolved = path
	}
	var target syscall.Stat_t
	if syscall.Lstat(resolved, &target) != nil {
		return false, false
	}
	for _, root := range []string{"/", "/System/Volumes/Data"} {
		var volume syscall.Stat_t
		if _, err := os.Stat(root); err != nil || syscall.Lstat(root, &volume) != nil {
			continue
		}
		if volume.Dev == target.Dev {
			return true, true
		}
	}
	return false, true
}
