package platform

import (
	"io/fs"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

// Raw sharing evidence never calls database policy (which itself calls IsOpen).
func IsOpen(path string) (bool, bool) {
	md, err := os.Lstat(path)
	if err != nil || md.Mode()&os.ModeSymlink != 0 {
		return false, false
	}
	if !md.IsDir() {
		return inUseFileOpen(path)
	}
	count := 0
	deadline := time.Now().Add(3 * time.Second)
	busy, known := false, true
	_ = filepath.WalkDir(path, func(p string, d fs.DirEntry, err error) error {
		count++
		if err != nil || count > 4096 || time.Now().After(deadline) || d.Type()&os.ModeSymlink != 0 {
			known = false
			return fs.SkipAll
		}
		if !d.IsDir() {
			busy, known = inUseFileOpen(p)
			if busy || !known {
				return fs.SkipAll
			}
		}
		return nil
	})
	return busy, known
}
func inUseFileOpen(path string) (bool, bool) {
	p, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return false, false
	}
	h, err := syscall.CreateFile(p, syscall.GENERIC_READ, 0, nil, syscall.OPEN_EXISTING, syscall.FILE_ATTRIBUTE_NORMAL, 0)
	if err == nil {
		syscall.CloseHandle(h)
		return false, true
	}
	if err == syscall.Errno(32) || err == syscall.Errno(33) {
		return true, true
	}
	return false, false
}

// The Rust scan-time Windows facade intentionally does not enumerate owners.
func DetectInUse(paths []string) map[string]InUseBusyInfo { return map[string]InUseBusyInfo{} }
