package platform

import (
	"errors"
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
	inUseProbeMu.RLock()
	err := removeFileExclusive(path)
	inUseProbeMu.RUnlock()
	if !errors.Is(err, windows.ERROR_SHARING_VIOLATION) {
		return err
	}
	// Parallel deletes and occupancy probes can hold another hardlink to
	// this file. Exclude all of our handles before reporting an external lock.
	inUseProbeMu.Lock()
	defer inUseProbeMu.Unlock()
	return removeFileExclusive(path)
}

func removeFileExclusive(path string) error {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	h, err := windows.CreateFile(p, windows.DELETE, 0, nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return &os.PathError{Op: "remove", Path: path, Err: err}
	}
	// FILE_DISPOSITION_INFO is a single BOOLEAN.
	del := byte(1)
	dispErr := windows.SetFileInformationByHandle(h, windows.FileDispositionInfo, &del, uint32(unsafe.Sizeof(del)))
	if dispErr == nil {
		_ = windows.CloseHandle(h)
		return nil
	}
	var info windows.ByHandleFileInformation
	readonly := windows.GetFileInformationByHandle(h, &info) == nil && info.FileAttributes&windows.FILE_ATTRIBUTE_READONLY != 0
	_ = windows.CloseHandle(h)
	if errors.Is(dispErr, windows.ERROR_USER_MAPPED_FILE) {
		return &os.PathError{Op: "remove", Path: path, Err: windows.ERROR_SHARING_VIOLATION}
	}
	if errors.Is(dispErr, windows.ERROR_ACCESS_DENIED) && !readonly && isMappedImageInUse(p) {
		return &os.PathError{Op: "remove", Path: path, Err: windows.ERROR_SHARING_VIOLATION}
	}
	return &os.PathError{Op: "remove", Path: path, Err: dispErr}
}

// A running executable or mapped DLL often has no open file handle once the PE
// loader finishes mapping it, so CreateFile(DELETE, share=0) succeeds while
// FileDispositionInfo fails with ERROR_ACCESS_DENIED via MmFlushImageSection.
func isMappedImageInUse(p *uint16) bool {
	wh, err := windows.CreateFile(p, windows.FILE_WRITE_DATA,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil, windows.OPEN_EXISTING, windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err == nil {
		_ = windows.CloseHandle(wh)
		return false
	}
	return errors.Is(err, windows.ERROR_SHARING_VIOLATION) || errors.Is(err, windows.ERROR_USER_MAPPED_FILE)
}
