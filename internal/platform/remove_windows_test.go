package platform

import (
	"errors"
	"os"
	"path/filepath"
	"syscall"
	"testing"

	"golang.org/x/sys/windows"
)

// A handle opened with FILE_SHARE_DELETE lets plain DeleteFile unlink the file
// under its owner. The exclusive delete must refuse it as in use.
func TestRemoveFileExclusiveRefusesHeldFilesEvenWithShareDelete(t *testing.T) {
	path := filepath.Join(t.TempDir(), "held.bin")
	if err := os.WriteFile(path, []byte("held"), 0600); err != nil {
		t.Fatal(err)
	}
	p, _ := windows.UTF16PtrFromString(path)
	h, err := windows.CreateFile(p, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, 0, 0)
	if err != nil {
		t.Fatal(err)
	}
	err = RemoveFileExclusive(path)
	if !errors.Is(err, syscall.Errno(32)) {
		t.Fatalf("held file: err=%v; want sharing violation", err)
	}
	if _, statErr := os.Lstat(path); statErr != nil {
		t.Fatalf("held file was unlinked: %v", statErr)
	}
	windows.CloseHandle(h)
	if err := RemoveFileExclusive(path); err != nil {
		t.Fatalf("released file: %v", err)
	}
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		t.Fatalf("released file still present: %v", err)
	}
}
