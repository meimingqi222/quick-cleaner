package platform

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// Linux is a migration test adapter, not a supported desktop backend.
func IsOpen(path string) (bool, bool) {
	md, err := os.Lstat(path)
	if err != nil || md.Mode()&os.ModeSymlink != 0 {
		return false, false
	}
	target, err := filepath.EvalSymlinks(path)
	if err != nil {
		return false, false
	}
	target, _ = filepath.Abs(target)
	procs, err := os.ReadDir("/proc")
	if err != nil {
		return false, false
	}
	known := true
	deadline := time.Now().Add(3 * time.Second)
	for _, proc := range procs {
		if _, err := strconv.Atoi(proc.Name()); err != nil {
			continue
		}
		fds, err := os.ReadDir(filepath.Join("/proc", proc.Name(), "fd"))
		if err != nil {
			if !os.IsNotExist(err) {
				known = false
			}
			continue
		}
		for _, fd := range fds {
			if time.Now().After(deadline) {
				return false, false
			}
			name, err := os.Readlink(filepath.Join("/proc", proc.Name(), "fd", fd.Name()))
			if err != nil {
				if !os.IsNotExist(err) {
					known = false
				}
				continue
			}
			name = strings.TrimSuffix(name, " (deleted)")
			if name == target || (md.IsDir() && strings.HasPrefix(name, target+string(os.PathSeparator))) {
				return true, true
			}
		}
	}
	return false, known
}
func DetectInUse(paths []string) map[string]InUseBusyInfo {
	out := make(map[string]InUseBusyInfo)
	for p, s := range SpotCheckInUse(paths) {
		if s != InUseClear {
			out[p] = InUseBusyInfo{Open: s == InUseBusy, Unknown: s == InUseUnknown}
		}
	}
	return out
}
