package platform

import (
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestRemoveFileExclusiveWaitsForOwnHardlinkProbe(t *testing.T) {
	path := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(path, []byte("cache"), 0600); err != nil {
		t.Fatal(err)
	}
	alias := path + "-alias"
	if err := os.Link(path, alias); err != nil {
		t.Fatal(err)
	}
	name, _ := windows.UTF16PtrFromString(alias)
	inUseProbeMu.RLock()
	h, err := windows.CreateFile(name, windows.GENERIC_READ, 0, nil, windows.OPEN_EXISTING, 0, 0)
	if err != nil {
		inUseProbeMu.RUnlock()
		t.Fatal(err)
	}
	result := make(chan error, 1)
	go func() { result <- RemoveFileExclusive(path) }()
	var premature bool
	select {
	case err = <-result:
		premature = true
	case <-time.After(50 * time.Millisecond):
	}
	windows.CloseHandle(h)
	inUseProbeMu.RUnlock()
	if premature {
		t.Fatalf("own probe became external occupancy: %v", err)
	}
	select {
	case err = <-result:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("delete did not resume after probe closed")
	}
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		t.Fatalf("file remains: %v", err)
	}
	if _, err := os.Lstat(alias); err != nil {
		t.Fatalf("other link lost: %v", err)
	}
}

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

func TestRemoveFileExclusiveClassifiesMappedImageAsSharingViolation(t *testing.T) {
	if os.Getenv("QC_MAPPED_HOLD_CHILD") == "1" {
		_, _ = io.Copy(io.Discard, os.Stdin)
		return
	}
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	src, err := os.ReadFile(self)
	if err != nil {
		t.Fatal(err)
	}
	running := filepath.Join(t.TempDir(), "cached-runner.exe")
	if err := os.WriteFile(running, src, 0700); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(running, "-test.run=^TestRemoveFileExclusiveClassifiesMappedImageAsSharingViolation$")
	cmd.Env = append(os.Environ(), "QC_MAPPED_HOLD_CHILD=1")
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = stdin.Close()
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
	})
	// Wait briefly so the PE loader finishes mapping the image section.
	time.Sleep(50 * time.Millisecond)
	err = RemoveFileExclusive(running)
	if !errors.Is(err, syscall.Errno(32)) {
		t.Fatalf("mapped running executable: err=%v; want sharing violation (errno 32)", err)
	}
	_ = stdin.Close()
	_ = cmd.Wait()
	if err := RemoveFileExclusive(running); err != nil {
		t.Fatalf("after process exit: %v", err)
	}
}
