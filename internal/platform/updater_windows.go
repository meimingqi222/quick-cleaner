package platform

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"unsafe"
)

func IsPackagedInstall() bool {
	exe, err := os.Executable()
	return err == nil && !LooksLikeDevBuild(exe)
}

func OpenURL(url string) error {
	if !strings.HasPrefix(url, "https://") && !strings.HasPrefix(url, "http://") {
		return fmt.Errorf("unsupported URL scheme")
	}
	p, err := syscall.UTF16PtrFromString(url)
	if err != nil {
		return err
	}
	verb, _ := syscall.UTF16PtrFromString("open")
	r, _, _ := shellExecute.Call(0, uintptr(unsafe.Pointer(verb)), uintptr(unsafe.Pointer(p)), 0, 0, 1)
	if r <= 32 {
		return fmt.Errorf("open URL: ShellExecuteW returned %d", r)
	}
	return nil
}

func CleanupPreviousUpdateLeftovers() {
	if exe, err := os.Executable(); err == nil {
		RemoveUpdateBackup(exe, false)
	}
}

// ApplyUpdateAndRestart returns after helper launch; the caller must then quit.
// Unlike bounded tool commands, this helper must outlive this process (P9).
func ApplyUpdateAndRestart(payload string) error {
	info, err := os.Stat(payload)
	if err != nil || !info.Mode().IsRegular() {
		return fmt.Errorf("update payload missing: %s", payload)
	}
	current, err := os.Executable()
	if err != nil {
		return fmt.Errorf("current exe: %w", err)
	}
	if current == payload {
		return fmt.Errorf("payload is the running executable")
	}
	if err := updateWriteProbe(current); err != nil {
		return err
	}
	cache, err := prepareUpdateCache()
	if err != nil {
		return err
	}
	script := filepath.Join(cache, "apply-update.ps1")
	if err := os.WriteFile(script, []byte(WindowsReplaceScript(current, payload, filepath.Join(cache, "apply-update.log"), os.Getpid())), 0600); err != nil {
		return fmt.Errorf("write helper: %w", err)
	}
	const base = 0x08000000 | 0x00000200 // NO_WINDOW | NEW_PROCESS_GROUP
	for _, flags := range []uint32{base | 0x01000000, base} {
		cmd := exec.Command("powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script)
		cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: flags, HideWindow: true}
		err = cmd.Start()
		if err == nil {
			return cmd.Process.Release()
		}
	}
	return fmt.Errorf("spawn update helper: %w", err)
}
