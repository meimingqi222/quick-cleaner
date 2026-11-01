package platform

import (
	"os"
	"unsafe"

	"golang.org/x/sys/windows"
)

// RemoveFileExclusive deletes a non-directory only if no other handle is open.
// Opening with share mode 0 is both the occupancy check and the delete handle,
// so there is no probe-then-delete window, and a file another process opened
// with FILE_SHARE_DELETE is refused instead of being unlinked under it.
// A sharing violation (errno 32) means "in use"; callers classify it.
func RemoveFileExclusive(path string) error {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	h, err := windows.CreateFile(p, windows.DELETE, 0, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	defer windows.CloseHandle(h)
	// FILE_DISPOSITION_INFO is a single BOOLEAN.
	del := byte(1)
	if err := windows.SetFileInformationByHandle(h, windows.FileDispositionInfo, &del, uint32(unsafe.Sizeof(del))); err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	return nil
}
