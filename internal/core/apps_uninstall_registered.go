package core

import (
	"errors"
	"fmt"
	"os"
	"runtime"
	"strings"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func appCommandStem(command string) string {
	exe, _ := SplitCommand(command)
	i := strings.LastIndexAny(exe, `/\`)
	file := exe[i+1:]
	if i := strings.LastIndexByte(file, '.'); i >= 0 {
		file = file[:i]
	}
	return asciiLower(file)
}

func appGenericHost(stem string) bool {
	switch stem {
	case "cmd", "powershell", "pwsh", "winget", "scoop", "choco", "chocolatey", "msiexec", "conhost", "explorer":
		return true
	}
	return false
}

func appCommandNeedsShell(command string) bool {
	if appGenericHost(appCommandStem(command)) {
		return true
	}
	exe, _ := SplitCommand(command)
	exe = asciiLower(exe)
	return strings.HasSuffix(exe, ".bat") || strings.HasSuffix(exe, ".cmd") || strings.HasSuffix(exe, ".ps1")
}

func appHiddenShell(command string) bool {
	switch appCommandStem(command) {
	case "msiexec", "winget", "scoop", "choco", "chocolatey":
		return true
	}
	return false
}

func prepareAppUninstallCommand(command string, app InstalledApp) string {
	out := strings.TrimSpace(command)
	has := func(flags ...string) bool {
		for _, token := range strings.Fields(asciiLower(command)) {
			for _, flag := range flags {
				if token == flag {
					return true
				}
			}
		}
		return false
	}
	switch appCommandStem(out) {
	case "winget":
		if !has("--scope") {
			switch app.RegistryRoot {
			case AppHKCU:
				out += " --scope user"
			case AppHKLM, AppHKLM32:
				out += " --scope machine"
			}
		}
		if !has("--disable-interactivity") {
			out += " --disable-interactivity"
		}
		if !has("--accept-source-agreements") {
			out += " --accept-source-agreements"
		}
		if !has("--silent", "-h") {
			out += " --silent"
		}
	case "choco", "chocolatey":
		if !has("-y", "--yes") {
			out += " -y"
		}
	}
	return out
}

type registeredUninstallRuntime struct {
	shell         func(string, bool, bool) (uint32, error)
	executable    func(string, []string) (uint32, error)
	elevated      func() bool
	registered    func(InstalledApp) bool
	processes     func(string, string) bool
	directoryGone func(InstalledApp) bool
	now           func() time.Time
	sleep         func(time.Duration)
}

func nativeRegisteredUninstallRuntime() registeredUninstallRuntime {
	return registeredUninstallRuntime{
		shell: platform.RunUninstallShell, executable: platform.RunUninstallExecutable,
		elevated: platform.IsElevated, processes: platform.HasRelatedAppProcess,
		registered: func(app InstalledApp) bool {
			return platform.AppRegistrationPresent(app.RegistryRoot.Label(), app.RegistrySubpath)
		},
		directoryGone: func(app InstalledApp) bool {
			if app.InstallLocation == nil || IsSystemRootDir(*app.InstallLocation) || len(Norm(*app.InstallLocation)) <= 3 {
				return false
			}
			_, err := os.Stat(*app.InstallLocation)
			return os.IsNotExist(err)
		}, now: time.Now, sleep: time.Sleep,
	}
}

// RunRegisteredAppUninstaller never deletes files itself. Vendor commands keep
// their original semantics; only observed installation state establishes success.
func RunRegisteredAppUninstaller(app InstalledApp) error {
	if runtime.GOOS != "windows" {
		return errors.New("Windows registered uninstall is unsupported on this platform")
	}
	return runRegisteredAppUninstaller(app, nativeRegisteredUninstallRuntime())
}

func runRegisteredAppUninstaller(app InstalledApp, r registeredUninstallRuntime) error {
	if app.Discovery != nil {
		return errors.New("discovered application requires its captured official operation")
	}
	if app.RegistryRoot != AppHKLM && app.RegistryRoot != AppHKLM32 && app.RegistryRoot != AppHKCU {
		return errors.New("application has no uninstall registration")
	}
	raw := app.QuietUninstallString
	if raw == nil {
		raw = app.UninstallString
	}
	if raw == nil {
		return errors.New("application has no uninstall command")
	}
	command := prepareAppUninstallCommand(*raw, app)
	stem := appCommandStem(*raw)
	unelevated := app.RegistryRoot == AppHKCU && (stem == "winget" || stem == "scoop" || stem == "choco" || stem == "chocolatey")
	var code uint32
	var err error
	if appCommandNeedsShell(command) {
		code, err = r.shell(command, appHiddenShell(command), unelevated)
		if err == nil && code == platform.WingetAdminContextProhibited && !unelevated && r.elevated() {
			code, err = r.shell(command, appHiddenShell(command), true)
		}
	} else {
		exe, args := SplitCommand(command)
		if exe == "" {
			return errors.New("invalid uninstall command")
		}
		code, err = r.executable(exe, args)
	}
	if err != nil {
		return err
	}
	installDir := ""
	if app.InstallLocation != nil && !IsSystemRootDir(*app.InstallLocation) {
		if n := Norm(*app.InstallLocation); len(n) > 3 {
			installDir = n
		}
	}
	if appGenericHost(stem) {
		stem = ""
	}
	if waitRegisteredUninstall(app, installDir, stem, code == 0, r) {
		return nil
	}
	if code == platform.WingetAdminContextProhibited {
		return errors.New("winget refused administrator context after desktop-user retry")
	}
	if code != 0 {
		return &UninstallerExitError{Code: code}
	}
	if !r.registered(app) {
		return ErrUninstallNotSettled
	}
	return &StillRegisteredError{Name: app.Name}
}

// UninstallerExitError is the official uninstaller's own failure. The exit code is
// kept typed so the UI can explain it instead of printing raw English.
type UninstallerExitError struct{ Code uint32 }

func (e *UninstallerExitError) Error() string {
	return fmt.Sprintf("uninstaller exited abnormally: exit code: %#x", e.Code)
}

// ErrUninstallNotSettled means the uninstaller exited but the installation has not
// settled yet; files may still be in use.
var ErrUninstallNotSettled = errors.New("uninstaller exited but installation did not settle; files may still be in use")

// StillRegisteredError means the uninstaller exited yet the entry is still listed.
type StillRegisteredError struct{ Name string }

func (e *StillRegisteredError) Error() string {
	return fmt.Sprintf("uninstaller exited but application remains registered: %s", e.Name)
}

func waitRegisteredUninstall(app InstalledApp, installDir, stem string, childOK bool, r registeredUninstallRuntime) bool {
	deadline := r.now().Add(30 * time.Minute)
	var unregisteredAt, orphanedAt *time.Time
	stable, sawProcesses := 0, false
	for r.now().Before(deadline) {
		registered := r.registered(app)
		processes := r.processes(installDir, stem)
		gone := r.directoryGone(app)
		now := r.now()
		if processes {
			sawProcesses = true
		}
		if registered {
			unregisteredAt = nil
		} else if unregisteredAt == nil {
			t := now
			unregisteredAt = &t
		}
		settled := !processes && (gone || !registered && unregisteredAt != nil && now.Sub(*unregisteredAt) >= 3*time.Second)
		// A successful parent can disappear before UAC. Only processes seen in
		// this loop count; a vanished second stage gets 120s, not the failed
		// command's 15s window (P8).
		limit := time.Duration(0)
		if !processes && registered && !gone {
			if sawProcesses {
				limit = 120 * time.Second
			} else if !childOK {
				limit = 15 * time.Second
			}
		}
		if limit == 0 {
			orphanedAt = nil
		} else {
			if orphanedAt == nil {
				t := now
				orphanedAt = &t
			}
			if now.Sub(*orphanedAt) >= limit {
				return false
			}
		}
		if settled {
			stable++
		} else {
			stable = 0
		}
		if stable >= 4 {
			return true
		}
		r.sleep(500 * time.Millisecond)
	}
	return false
}

func RunRegisteredAppUninstallerReported(app InstalledApp) UninstallOutcome {
	return runRegisteredAppUninstallerReported(app, RunRegisteredAppUninstaller, func(app InstalledApp) bool {
		return platform.AppRegistrationPresent(app.RegistryRoot.Label(), app.RegistrySubpath)
	})
}

func runRegisteredAppUninstallerReported(app InstalledApp, run func(InstalledApp) error, registered func(InstalledApp) bool) UninstallOutcome {
	anchor := app.RegistrySubpath
	var identity *TargetIdentity
	if app.InstallLocation != nil {
		anchor = *app.InstallLocation
		if id, ok := CaptureIdentity(anchor); ok {
			identity = &id
		}
	}
	plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{{Path: anchor, Operation: Operation{Kind: OpOfficialUninstall}, Identity: identity, Disposal: Permanent}})
	report := ExecuteRegisteredPlan(plan, 0, NewCleanProgress(1, 0), func() error { return run(app) }, func(CompletionCondition) Evidence {
		if registered(app) {
			return Absent
		}
		return Confirmed
	})
	return UninstallOutcome{Result: ExecutionResult(report), PlanExecutions: report.PlanExecutions}
}
