package core

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"
	"unicode/utf8"
)

type sourceInstallRuntime struct {
	idle, official, registrations func() error
	shortcutTargets               func(string, string) bool
	clean                         func([]ArbitraryTarget) CleanReport
	now                           func() time.Time
	sleep                         func(time.Duration)
}

func sourceArtifactsRemoved(paths []string) bool {
	if len(paths) == 0 {
		return false
	}
	for _, path := range paths {
		if _, err := os.Lstat(path); !os.IsNotExist(err) {
			return false
		}
	}
	return true
}

func (p *sourceInstallPlan) execute(scanned *CleanupPlan, shortcuts []installationArtifact, executable string, r sourceInstallRuntime, settle *OfficialUninstaller, budget time.Duration) UninstallOutcome {
	progress := NewCleanProgress(0, 0)
	report := ExecuteSourcePlan(scanned, 0, progress, func(action SourceAction) error {
		switch action {
		case "revalidate":
			if err := r.idle(); err != nil {
				return err
			}
			return p.recheckShared()
		case "official_operation":
			return r.official()
		case "supplement_cleanup":
			if scanned.installation == nil {
				return errors.New("missing scanned installation instance")
			}
			if err := scanned.installation.ValidateLivePaths(filepath.Join(p.home, p.rule.Source), p.observedPaths()); err != nil {
				return err
			}
			return p.supplement(scanned.installation, shortcuts, executable, r)
		case "verify_completion":
			if err := p.verify(r); err != nil {
				return err
			}
			if settle != nil {
				return p.awaitStable(r, *settle, budget)
			}
			return nil
		case "remove_recovery_records":
			if err := r.idle(); err != nil {
				return err
			}
			if err := p.recheckShared(); err != nil {
				return err
			}
			if err := scanned.installation.Revalidate(); err != nil {
				return err
			}
			return p.removeRecovery(r)
		default:
			return errors.New("unsupported source lifecycle action")
		}
	})
	return UninstallOutcome{Result: ExecutionResult(report), PlanExecutions: report.PlanExecutions}
}

func (p *sourceInstallPlan) supplement(instance *InstallationInstance, shortcuts []installationArtifact, executable string, r sourceInstallRuntime) error {
	if err := r.idle(); err != nil {
		return err
	}
	if err := p.recheckShared(); err != nil {
		return err
	}
	if err := p.removeStateContents(r); err != nil {
		return err
	}
	for _, entry := range p.startup {
		id, err := instance.ScannedIdentity(entry.path)
		if err != nil {
			return err
		}
		if id == nil {
			continue
		}
		data, readErr := sourceReadFile(entry.path, 4*1024*1024)
		if entry.identity == nil || !entry.identity.Recheck(entry.path) || readErr != nil || !utf8.Valid(data) || !sourceGatewayReference(string(data), p.home, p.rule) {
			return errors.New("Gateway Startup entry changed")
		}
		if err := p.removePaths([]string{entry.path}, false, r); err != nil {
			return err
		}
	}
	if err := r.registrations(); err != nil {
		return err
	}
	for _, entry := range shortcuts {
		id, err := instance.ScannedIdentity(entry.path)
		if err != nil {
			return err
		}
		if id == nil {
			continue
		}
		if entry.identity == nil || !entry.identity.Recheck(entry.path) || !r.shortcutTargets(entry.path, executable) {
			return fmt.Errorf("shortcut changed: %s", entry.path)
		}
		if err := p.removePaths([]string{entry.path}, false, r); err != nil {
			return err
		}
	}
	return p.removePaths(p.beforeRecoveryArtifacts(p.paths), true, r)
}

func (p *sourceInstallPlan) beforeRecoveryArtifacts(paths []string) []string {
	facts := filepath.Join(p.home, p.rule.ToolsDir, p.rule.ToolFacts)
	var out []string
	for _, path := range paths {
		if path != p.state && path != facts {
			out = append(out, path)
		}
	}
	return out
}

func (p *sourceInstallPlan) verify(r sourceInstallRuntime) error {
	if err := r.idle(); err != nil {
		return err
	}
	if err := p.recheckShared(); err != nil {
		return err
	}
	if err := r.registrations(); err != nil {
		return err
	}
	if !sourceArtifactsRemoved(p.beforeRecoveryArtifacts(p.paths)) {
		return errors.New("program artifacts remain before recovery cleanup")
	}
	return nil
}

func (p *sourceInstallPlan) awaitStable(r sourceInstallRuntime, command OfficialUninstaller, budget time.Duration) error {
	artifacts := p.beforeRecoveryArtifacts(command.InstalledArtifacts)
	deadline := r.now().Add(budget)
	stable := 0
	for {
		if sourceArtifactsRemoved(artifacts) && r.idle() == nil && p.recheckShared() == nil {
			stable++
		} else {
			stable = 0
		}
		if stable >= 4 {
			return nil
		}
		if !r.now().Before(deadline) {
			return fmt.Errorf("official uninstall incomplete: %s", command.Provider)
		}
		r.sleep(500 * time.Millisecond)
	}
}

func (p *sourceInstallPlan) removeStateContents(r sourceInstallRuntime) error {
	if p.state == "" || sourceArtifactsRemoved([]string{p.state}) {
		return nil
	}
	if !sourceOwnedPath(p.home, p.state) {
		return errors.New("installation receipt redirected")
	}
	record := Norm(filepath.Join(p.state, p.rule.StateFacts))
	ancestors := []string{p.state}
	count := 0
	var paths []string
	for len(ancestors) > 0 {
		directory := ancestors[len(ancestors)-1]
		ancestors = ancestors[:len(ancestors)-1]
		f, err := os.Open(directory)
		if err != nil {
			return err
		}
		entries, err := f.ReadDir(257)
		f.Close()
		if err != nil && err != io.EOF {
			return err
		}
		for _, entry := range entries {
			count++
			if count > 256 {
				return errors.New("installation receipt enumeration limit")
			}
			path := filepath.Join(directory, entry.Name())
			if !sourceOwnedPath(p.home, path) {
				return errors.New("installation state child redirected")
			}
			if Norm(path) == record {
				continue
			}
			if atOrUnder(record, Norm(path)) {
				rel, err := filepath.Rel(p.state, path)
				if err != nil {
					return err
				}
				if len(strings.Split(rel, string(filepath.Separator))) > 32 {
					return errors.New("installation receipt ancestor depth limit")
				}
				ancestors = append(ancestors, path)
			} else {
				paths = append(paths, path)
			}
		}
	}
	return p.removePaths(paths, true, r)
}

func (p *sourceInstallPlan) removeRecovery(r sourceInstallRuntime) error {
	facts := filepath.Join(p.home, p.rule.ToolsDir, p.rule.ToolFacts)
	if slices.Contains(p.paths, facts) {
		if err := p.removePaths([]string{facts}, true, r); err != nil {
			return err
		}
	}
	if p.state != "" {
		if err := p.removePaths([]string{p.state}, true, r); err != nil {
			return err
		}
	}
	if !sourceArtifactsRemoved(p.paths) {
		return errors.New("program artifacts remain")
	}
	return nil
}

func (p *sourceInstallPlan) removePaths(paths []string, requireOwned bool, r sourceInstallRuntime) error {
	var targets []ArbitraryTarget
	for _, path := range paths {
		if sourceArtifactsRemoved([]string{path}) {
			continue
		}
		if requireOwned && !sourceOwnedPath(p.home, path) {
			return fmt.Errorf("artifact changed: %s", path)
		}
		target := CaptureArbitraryTarget(path)
		if target.Identity == nil {
			return fmt.Errorf("unverified artifact: %s", path)
		}
		targets = append(targets, target)
	}
	report := r.clean(targets)
	if len(report.Failed) > 0 || len(report.SkippedItems) > 0 || len(report.Manual) > 0 {
		return errors.New("program cleanup incomplete; see cleanup log")
	}
	for _, target := range targets {
		if !sourceArtifactsRemoved([]string{target.Path}) {
			return errors.New("program cleanup did not remove artifacts")
		}
	}
	return nil
}
