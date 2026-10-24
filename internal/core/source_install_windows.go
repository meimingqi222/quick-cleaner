package core

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func sourcePowerShell() string {
	root := os.Getenv("SystemRoot")
	if root == "" {
		return ""
	}
	return filepath.Join(root, `System32\WindowsPowerShell\v1.0\powershell.exe`)
}

func nativeInspectSourceApp(target string, snapshot *RuleSnapshot) *InstalledApp {
	roaming, _ := platform.UserDataDir()
	return inspectSourceApp(target, snapshot, platform.CanonicalAppPath, roaming, sourcePowerShell())
}

func RunDiscoveredAppUninstallerReported(app InstalledApp) UninstallOutcome {
	out := runSourceAppUninstaller(app)
	if out.Result != nil && len(out.PlanExecutions) == 0 && app.Discovery != nil && app.Discovery.Plan != nil {
		report := ExecuteSourcePlan(app.Discovery.Plan, 0, NewCleanProgress(0, 0), func(SourceAction) error { return out.Result })
		out.PlanExecutions = report.PlanExecutions
	}
	return out
}

func RunDiscoveredAppUninstaller(app InstalledApp) error {
	return RunDiscoveredAppUninstallerReported(app).Result
}

func runSourceAppUninstaller(app InstalledApp) UninstallOutcome {
	fail := func(err error) UninstallOutcome { return UninstallOutcome{Result: err} }
	if app.Discovery == nil || app.Discovery.Plan == nil {
		return fail(errors.New("missing scanned installation plan"))
	}
	scanned := app.Discovery.Plan
	if err := scanned.Validate(); err != nil {
		return fail(err)
	}
	live := nativeInspectSourceApp(app.Discovery.Executable, scanned.rule.origin)
	if live == nil || live.Discovery == nil || live.Discovery.Rule == nil || scanned.rule.id != live.Discovery.Rule.id || scanned.rule.origin != live.Discovery.Rule.origin {
		return fail(errors.New("installation evidence or rule changed"))
	}
	if scanned.official == nil {
		return fail(errors.New("missing scanned official operation"))
	}
	command, err := scanned.official.Command()
	if err != nil {
		return fail(err)
	}
	rule := scanned.rule.snapshot.Definition(scanned.rule.id)
	if rule == nil || rule.App == nil {
		return fail(errors.New("missing installation rule"))
	}
	if scanned.installation == nil {
		return fail(errors.New("missing scanned installation instance"))
	}
	code := scanned.installation.root
	owner, ownerErr := os.Lstat(filepath.Join(code, rule.App.SourceOwner))
	roaming, _ := platform.UserDataDir()
	p, err := buildSourceInstallPlan(code, ownerErr == nil && !ruleEvidenceLink(owner), *rule.App, platform.CanonicalAppPath, roaming)
	if err != nil {
		return fail(err)
	}
	if err := scanned.installation.ValidateLivePaths(code, p.observedPaths()); err != nil {
		return fail(err)
	}
	shortcutTargets := func(path, target string) bool {
		value, ok := platform.ReadWindowsShortcutTarget(path)
		if !ok {
			return false
		}
		value, ok = canonicalSourceTarget(value, platform.CanonicalAppPath)
		return ok && Norm(value) == Norm(target)
	}
	var shortcuts []installationArtifact
	for _, path := range app.Discovery.Shortcuts {
		if !shortcutTargets(path, app.Discovery.Executable) {
			continue
		}
		identity, err := scanned.installation.ScannedIdentity(path)
		if err != nil {
			return fail(err)
		}
		if identity != nil {
			shortcuts = append(shortcuts, installationArtifact{path: path, identity: identity})
		}
	}
	r := sourceInstallRuntime{shortcutTargets: shortcutTargets, now: time.Now, sleep: time.Sleep,
		clean: func(items []ArbitraryTarget) CleanReport {
			return CleanArbitraryItems(items, Permanent, NewCleanProgress(0, 0))
		},
		idle: func() error {
			processes, err := platform.ReadWindowsAppProcesses()
			return sourceInstallIdle(p.home, p.rule, processes, err)
		},
		official: func() error {
			if p.sharedTools {
				return nil
			}
			frozen, err := scanned.official.Command()
			if err != nil {
				return err
			}
			exit, err := platform.RunOfficialAppCommand(frozen.Executable, frozen.Arguments, frozen.WorkingDirectory)
			if err != nil {
				return err
			}
			if exit != 0 {
				return fmt.Errorf("official uninstall failed (exit %#x)", exit)
			}
			return nil
		},
		registrations: func() error {
			powershell := sourcePowerShell()
			if powershell == "" {
				return errors.New("missing SystemRoot")
			}
			exit, err := platform.RunOfficialAppCommand(powershell, []string{"-NoProfile", "-NonInteractive", "-Command", sourceRegistrationScript(p.home, "Environment", !p.sharedTools, true, p.rule)}, p.home)
			if err != nil {
				return err
			}
			if exit != 0 {
				return fmt.Errorf("registration cleanup/verification failed (%d)", exit)
			}
			return nil
		},
	}
	return p.execute(scanned, shortcuts, app.Discovery.Executable, r, &command, 120*time.Second)
}
