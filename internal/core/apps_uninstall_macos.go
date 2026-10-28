package core

import (
	"context"
	"errors"
	"fmt"
	"os"
	"runtime"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func RunMacAppUninstaller(app InstalledApp) error {
	if runtime.GOOS != "darwin" {
		return errors.New("macOS application uninstall is not supported on this platform")
	}
	return runMacAppUninstallerWith(app, func(app InstalledApp) (bool, error) {
		return RunAppCaskUninstaller(context.Background(), app)
	}, func(path string) error {
		run, ok := RunWithTimeout("/usr/bin/open", []string{"--wait-apps", path}, 30*time.Minute)
		if !ok {
			return errors.New("embedded uninstaller unavailable or timed out; no fallback deletion performed")
		}
		if !run.OK {
			return fmt.Errorf("embedded uninstaller failed: %s", strings.TrimSpace(strings.ToValidUTF8(string(run.Stderr), "\uFFFD")))
		}
		return nil
	}, platform.MoveToTrash)
}

func runMacAppUninstallerWith(app InstalledApp, brew func(InstalledApp) (bool, error), open func(string) error, trash func(string) error) error {
	if RequiresManualUninstall(app) {
		return errors.New("complete uninstall in the official application settings; direct deletion was not performed")
	}
	if handled, err := RunReviewedScriptUninstaller(app); handled {
		return err
	}
	if app.InstallLocation == nil {
		return errors.New("application path unavailable")
	}
	location := *app.InstallLocation
	if _, err := os.Stat(location); err != nil {
		return fmt.Errorf("application path unavailable: %w", err)
	}
	if location == "/System" || strings.HasPrefix(location, "/System/") {
		return errors.New("system application on the read-only system volume cannot be uninstalled")
	}
	if handled, err := brew(app); err != nil || handled {
		return err
	}
	snapshot, err := CurrentRules()
	if err != nil {
		return err
	}
	if app.UninstallString == nil {
		if rule := snapshot.Definition("residual-macos"); rule != nil {
			if _, declared := rule.Locations["embedded_uninstaller:"+app.RegistrySubpath]; declared {
				return errors.New("required official uninstaller unavailable; no fallback deletion performed")
			}
		}
	} else {
		uninstaller := *app.UninstallString
		confined, err := ConfinedRulePath(location, uninstaller)
		info, statErr := os.Stat(uninstaller)
		if err != nil || !confined || statErr != nil || !info.IsDir() {
			return errors.New("embedded uninstaller missing or redirected; no fallback deletion performed")
		}
		if err := open(uninstaller); err != nil {
			return err
		}
		if !waitForAppBundleRemoval(location) {
			return fmt.Errorf("embedded uninstaller exited but application still exists or cannot be verified: %s", location)
		}
		return nil
	}
	if err := trash(location); err != nil {
		return err
	}
	if _, err := os.Lstat(location); !os.IsNotExist(err) {
		return fmt.Errorf("application still exists or cannot be verified after trash: %s", location)
	}
	return nil
}

func waitForAppBundleRemoval(path string) bool {
	for range 50 {
		if _, err := os.Lstat(path); err != nil {
			return os.IsNotExist(err)
		}
		time.Sleep(100 * time.Millisecond)
	}
	_, err := os.Lstat(path)
	return os.IsNotExist(err)
}
