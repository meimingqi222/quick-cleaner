package platform

import (
	"os"
	"syscall"
)

// DevEnvStorageFileInfo obtains link count and identity in one handle query.
// ObjectID is intentionally not used: its deletion-specific ancestor checks
// reject relative paths and do not expose the link count.
func DevEnvStorageFileInfo(path string) (uint32, [2]uint64, bool) {
	wide, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		return 0, [2]uint64{}, false
	}
	handle, err := syscall.CreateFile(wide, objectReadAttributes,
		syscall.FILE_SHARE_READ|syscall.FILE_SHARE_WRITE|syscall.FILE_SHARE_DELETE,
		nil, syscall.OPEN_EXISTING, syscall.FILE_FLAG_BACKUP_SEMANTICS|syscall.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return 0, [2]uint64{}, false
	}
	defer syscall.CloseHandle(handle)
	var info syscall.ByHandleFileInformation
	if syscall.GetFileInformationByHandle(handle, &info) != nil {
		return 0, [2]uint64{}, false
	}
	return info.NumberOfLinks, [2]uint64{uint64(info.VolumeSerialNumber), uint64(info.FileIndexHigh)<<32 | uint64(info.FileIndexLow)}, true
}

// DevEnvStorageIsReparse also rejects junctions and other non-symlink reparse tags.
func DevEnvStorageIsReparse(info os.FileInfo) bool {
	data, ok := info.Sys().(*syscall.Win32FileAttributeData)
	return info.Mode()&os.ModeSymlink != 0 || ok && data.FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0
}
