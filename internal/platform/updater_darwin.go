package platform

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
)

func IsPackagedInstall() bool {
	exe, err := os.Executable()
	_, bundle := UpdateBundleForExecutable(exe)
	return err == nil && bundle && !LooksLikeDevBuild(exe)
}

func OpenURL(url string) error {
	if !strings.HasPrefix(url, "https://") && !strings.HasPrefix(url, "http://") {
		return fmt.Errorf("unsupported URL scheme")
	}
	cmd := exec.Command("/usr/bin/open", url)
	if err := cmd.Start(); err != nil {
		return err
	}
	go func() { _ = cmd.Wait() }()
	return nil
}

func CleanupPreviousUpdateLeftovers() {
	if exe, err := os.Executable(); err == nil {
		if bundle, ok := UpdateBundleForExecutable(exe); ok {
			RemoveUpdateBackup(bundle, true)
		}
	}
}

// ApplyUpdateAndRestart does not exit the caller; the UI quits after success.
func ApplyUpdateAndRestart(payload string) error {
	info, err := os.Stat(payload)
	if err != nil || !info.IsDir() {
		return fmt.Errorf("update payload missing: %s", payload)
	}
	exe, err := os.Executable()
	if err != nil {
		return fmt.Errorf("current exe: %w", err)
	}
	current, ok := UpdateBundleForExecutable(exe)
	if !ok {
		return fmt.Errorf("not running from a .app bundle")
	}
	if err := updateWriteProbe(current); err != nil {
		return err
	}
	cache, err := prepareUpdateCache()
	if err != nil {
		return err
	}
	script := filepath.Join(cache, "apply-update.sh")
	if err := os.WriteFile(script, []byte(MacosBundleReplaceScript(current, payload, filepath.Join(cache, "apply-update.log"), os.Getpid())), 0700); err != nil {
		return fmt.Errorf("write helper: %w", err)
	}
	cmd := exec.Command("/bin/bash", script)
	// A new session plus the script's HUP trap protects the rename/copy interval.
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("spawn update helper: %w", err)
	}
	return cmd.Process.Release()
}
