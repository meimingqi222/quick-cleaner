package platform

import (
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

const objectReadAttributes = 0x0080 // FILE_READ_ATTRIBUTES (absent from syscall).

// ObjectID returns the volume serial and file index. Unlike core's weak
// metadata fallback this API refuses reparse points anywhere along the path.
func ObjectID(path string) ([2]uint64, bool) {
	return objectID(path, os.Lstat)
}

// ObjectIDUsing is ObjectID with a caller-supplied Lstat. A discovery scan
// passes a cache so shared ancestors are not stated again. nil uses os.Lstat.
func ObjectIDUsing(path string, lstat func(string) (os.FileInfo, error)) ([2]uint64, bool) {
	if lstat == nil {
		lstat = os.Lstat
	}
	return objectID(path, lstat)
}

func objectID(path string, lstat func(string) (os.FileInfo, error)) ([2]uint64, bool) {
	var none [2]uint64
	// Rust's ancestors iterator ends relative paths with the empty path;
	// symlink_metadata("") fails, so they cannot yield a stable identity.
	if !filepath.IsAbs(path) {
		return none, false
	}
	ancestors := []string{}
	volume := filepath.VolumeName(path)
	root := volume + `\`
	for current := strings.TrimRight(path, `\/`); ; {
		if len(current) <= len(volume) {
			current = root
		}
		ancestors = append(ancestors, current)
		if len(ancestors) > 128 {
			return none, false
		}
		if current == root {
			break
		}
		// Do not clean away '..': its preceding component may be a reparse
		// point and must still be checked, just as with Rust Path::ancestors.
		end := strings.LastIndexAny(current, `\/`)
		if end < len(volume) {
			current = root
		} else {
			current = strings.TrimRight(current[:end], `\/`)
		}
	}
	for _, ancestor := range ancestors {
		info, err := lstat(ancestor)
		if err != nil {
			return none, false
		}
		data, ok := info.Sys().(*syscall.Win32FileAttributeData)
		if !ok || data.FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return none, false
		}
	}
	wide, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return none, false
	}
	handle, err := syscall.CreateFile(wide, objectReadAttributes,
		syscall.FILE_SHARE_READ|syscall.FILE_SHARE_WRITE|syscall.FILE_SHARE_DELETE,
		nil, syscall.OPEN_EXISTING, syscall.FILE_FLAG_BACKUP_SEMANTICS|syscall.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return none, false
	}
	defer syscall.CloseHandle(handle)
	var info syscall.ByHandleFileInformation
	if syscall.GetFileInformationByHandle(handle, &info) != nil || info.FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
		return none, false
	}
	return [2]uint64{uint64(info.VolumeSerialNumber), uint64(info.FileIndexHigh)<<32 | uint64(info.FileIndexLow)}, true
}
