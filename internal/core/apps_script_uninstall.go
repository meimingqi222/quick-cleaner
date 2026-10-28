package core

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

const elevatedAppUninstallScript = `on run argv
    set userHome to item 1 of argv as text
    set interpreter to item 2 of argv as text
    set scriptBody to item 3 of argv as text
    set cmd to "/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME=" & quoted form of userHome & " " & quoted form of interpreter & " -c " & quoted form of scriptBody & " uninstall"
    repeat with i from 4 to count of argv
        set cmd to cmd & " " & quoted form of (item i of argv as text)
    end repeat
    do shell script cmd with administrator privileges
end run`

// A recognized script route owns failures as well as successes. Callers must
// never use an error as permission to fall back to deleting the bundle.
func RunReviewedScriptUninstaller(app InstalledApp) (handled bool, err error) {
	spec := ReviewedScriptUninstaller(app.RegistrySubpath)
	if spec == nil {
		return false, nil
	}
	home, ok := platform.UserHome()
	if !ok {
		return true, errors.New("original user home unavailable")
	}
	locations := make([]string, 0, len(spec.SystemBundles)+len(spec.UserBundles))
	for _, path := range spec.SystemBundles {
		locations = append(locations, filepath.Join(string(filepath.Separator), filepath.FromSlash(path)))
	}
	for _, path := range spec.UserBundles {
		locations = append(locations, filepath.Join(home, filepath.FromSlash(path)))
	}
	return true, runReviewedScriptIn(app, *spec, home, locations, platform.ReadBundleMetadata, executeReviewedAppScript)
}

func runReviewedScriptIn(app InstalledApp, spec ScriptUninstaller, home string, locations []string, metadata func(string) (*string, *string), execute func(string, ScriptUninstaller, string, bool) error) error {
	if app.InstallLocation == nil || !member(*app.InstallLocation, locations) {
		return errors.New("application is outside the reviewed installation locations; no fallback deletion performed")
	}
	elevated := false
	for _, path := range locations {
		info, err := os.Lstat(path)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil || !info.IsDir() || ruleEvidenceLink(info) {
			return fmt.Errorf("application path cannot be verified: %s", path)
		}
		userInstall := pathWithin(path, home)
		anchor := home
		if !userInstall {
			anchor = ""
			for parent := filepath.Dir(path); ; parent = filepath.Dir(parent) {
				if name := filepath.Base(parent); name == "Applications" || name == "Library" {
					anchor = parent
				}
				if parent == filepath.Dir(parent) {
					break
				}
			}
		}
		plist := filepath.Join(path, "Contents", "Info.plist")
		confined, pathErr := ConfinedRulePath(anchor, path)
		confinedPlist, plistErr := ConfinedRuleFile(path, plist)
		if anchor == "" || IsProtectedResidualPath(path) || pathErr != nil || !confined || plistErr != nil || !confinedPlist {
			return fmt.Errorf("application identity or path cannot be verified: %s", path)
		}
		id, _ := metadata(plist)
		if id == nil || *id != spec.BundleID {
			return fmt.Errorf("application bundle identity changed: %s", path)
		}
		elevated = elevated || !userInstall
	}
	bundle := *app.InstallLocation
	id, _ := metadata(filepath.Join(bundle, "Contents", "Info.plist"))
	if id == nil || *id != spec.BundleID {
		return errors.New("reviewed application identity unavailable")
	}
	script := filepath.Join(bundle, filepath.FromSlash(spec.Script))
	if confined, err := ConfinedRuleFile(bundle, script); err != nil || !confined {
		return errors.New("reviewed official script unavailable")
	}
	body, err := os.ReadFile(script)
	if err != nil {
		return err
	}
	if !member(fmt.Sprintf("%x", sha256.Sum256(body)), spec.SHA256) {
		return errors.New("official script version has not been reviewed; no fallback deletion performed")
	}
	if !utf8.Valid(body) {
		return errors.New("official script is not valid UTF-8")
	}
	if err := execute(string(body), spec, home, elevated); err != nil {
		return err
	}
	for _, path := range locations {
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			return fmt.Errorf("application still exists or cannot be verified after uninstall: %s", path)
		}
	}
	return nil
}

func executeReviewedAppScript(body string, spec ScriptUninstaller, home string, elevated bool) error {
	interpreter := "/bin/sh"
	if spec.Interpreter == "bash" {
		interpreter = "/bin/bash"
	} else if spec.Interpreter != "sh" {
		return errors.New("unreviewed script interpreter")
	}
	program := "/usr/bin/env"
	args := []string{"-i", "PATH=/usr/bin:/bin:/usr/sbin:/sbin", "HOME=" + home, interpreter, "-c", body, "uninstall"}
	if elevated {
		program = "/usr/bin/osascript"
		args = []string{"-e", elevatedAppUninstallScript, "--", home, interpreter, body}
	}
	args = append(args, spec.Arguments...)
	result, ok := RunWithTimeout(program, args, 30*time.Minute)
	if !ok {
		return errors.New("official uninstall unavailable or timed out; no fallback deletion performed")
	}
	if !result.OK {
		return fmt.Errorf("official uninstall failed: %s", strings.TrimSpace(strings.ToValidUTF8(string(result.Stderr), "\uFFFD")))
	}
	return nil
}
