//go:build linux

package platform

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
)

// Linux is a migration host, not a supported product platform. In particular,
// do not emulate trash by renaming into an arbitrary HOME/XDG directory.
func MoveToTrash(path string) error {
	if _, err := trashPath(path); err != nil {
		return err
	}
	return ErrTrashUnsupported
}
func currentUID() string { return strconv.Itoa(os.Getuid()) }
func IsVolumeTrash(path string) bool {
	parent := filepath.Dir(path)
	return filepath.Base(parent) == ".Trashes" && filepath.Base(path) == currentUID() && strings.HasPrefix(path, "/Volumes/")
}
func EmptyTrashShell() error         { return ErrTrashUnsupported }
func trashDirectories() []string     { return nil }
func IsPerDriveTrash(string) bool    { return false }
func IsSystemTrash(string) bool      { return false }
func IsRecycleJunkEntry(string) bool { return true }
func ForceDeleteAccess(string) bool  { return false }

// AllocatedFileSize reports allocated 512-byte blocks, including sparse files.
func AllocatedFileSize(info os.FileInfo) uint64 {
	if info != nil {
		if s, ok := info.Sys().(*syscall.Stat_t); ok && s.Blocks >= 0 {
			return uint64(s.Blocks) * 512
		}
	}
	return logicalFileSize(info)
}
