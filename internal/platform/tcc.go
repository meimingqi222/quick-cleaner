package platform

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"syscall"
	"time"
)

func IsTCCDenied(err error) bool { return errors.Is(err, syscall.EPERM) }

func HasFullDiskAccess() bool {
	home, ok := UserHome()
	return ok && ProbeFullDiskAccess(home)
}

// ProbeFullDiskAccess uses real opens, never metadata as authorization evidence.
// The first existing probe decides; missing probes alone prove nothing.
func ProbeFullDiskAccess(home string) bool {
	for _, relative := range []string{"Library/Application Support/com.apple.TCC/TCC.db", "Library/Mail", "Library/Safari"} {
		file, err := os.Open(filepath.Join(home, relative))
		if err == nil {
			file.Close()
			return true
		}
		if !os.IsNotExist(err) {
			return false
		}
	}
	return false
}

func AppBundleForExecutable(exe string) (string, bool) {
	parent := exe
	for range 3 {
		next := filepath.Dir(parent)
		if next == parent || next == "." {
			return "", false
		}
		parent = next
	}
	return parent, filepath.Ext(parent) == ".app"
}

func EnclosingAppBundle() (string, bool) {
	exe, err := os.Executable()
	if err != nil {
		return "", false
	}
	return AppBundleForExecutable(exe)
}

func OpenFullDiskAccessSettings() error {
	return openMacSettings("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
}

func OpenLoginItemsSettings() error {
	return openMacSettings("x-apple.systempreferences:com.apple.LoginItems-Settings.extension")
}

func openMacSettings(url string) error {
	if runtime.GOOS != "darwin" {
		return errors.New("macOS settings are unavailable on this platform")
	}
	result, complete := RunCommandTree(exec.Command("/usr/bin/open", url), 5*time.Second)
	if !complete || !result.OK {
		return errors.New("could not open macOS settings")
	}
	return nil
}
