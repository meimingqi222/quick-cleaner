package core

import (
	"errors"
	"os"
	"runtime"
)

type installationArtifact struct {
	path     string
	identity *TargetIdentity
}

func captureInstallationArtifact(path string) (installationArtifact, error) {
	a := installationArtifact{path: path}
	md, err := os.Lstat(path)
	if os.IsNotExist(err) {
		return a, nil
	}
	if err != nil {
		return a, err
	}
	if ruleEvidenceLink(md) {
		return a, errors.New("Redirected installation artifact")
	}
	id, ok := CaptureIdentity(path)
	if !ok {
		return a, errors.New("Installation artifact identity unavailable")
	}
	if runtime.GOOS == "windows" && !id.hasStable {
		return a, errors.New("Installation stable identity unavailable")
	}
	a.identity = &id
	return a, nil
}

func (a installationArtifact) unchanged() bool {
	if a.identity == nil {
		return false
	}
	if runtime.GOOS == "windows" {
		now, ok := CaptureIdentity(a.path)
		return ok && now.hasStable && a.identity.hasStable && now.stable == a.identity.stable
	}
	return a.identity.Recheck(a.path)
}

type InstallationInstance struct {
	root      string
	artifacts []installationArtifact
}

func CaptureInstallation(root string, paths []string) (*InstallationInstance, error) {
	if len(paths) == 0 || len(paths) > 4096 {
		return nil, errors.New("Installation artifact budget invalid")
	}
	instance := &InstallationInstance{root: root}
	seen := make(map[string]bool)
	for _, path := range paths {
		a, err := captureInstallationArtifact(path)
		if err != nil {
			return nil, err
		}
		if !seen[Norm(path)] {
			instance.artifacts = append(instance.artifacts, a)
			seen[Norm(path)] = true
		}
	}
	return instance, nil
}
func (i *InstallationInstance) ArtifactCount() int { return len(i.artifacts) }
func (i *InstallationInstance) Revalidate() error {
	for _, a := range i.artifacts {
		if _, err := os.Lstat(a.path); os.IsNotExist(err) {
			continue
		}
		if !a.unchanged() {
			return errors.New("Installation artifact changed, appeared or became unreadable since scan")
		}
	}
	return nil
}
func (i *InstallationInstance) ValidateLivePaths(root string, paths []string) error {
	if Norm(root) != Norm(i.root) {
		return errors.New("Installation instance changed since scan")
	}
	for _, path := range paths {
		seen := false
		for _, a := range i.artifacts {
			if Norm(a.path) == Norm(path) {
				seen = true
				break
			}
		}
		if !seen {
			return errors.New("Installation scope expanded since scan")
		}
	}
	return i.Revalidate()
}

// ObserveScanPaths is only for assembling the scan result. Execution must use
// ValidateLivePaths instead, so newly discovered paths cannot expand authority.
func (i *InstallationInstance) ObserveScanPaths(paths []string) error {
	if len(paths) == 0 {
		return nil
	}
	additions, err := CaptureInstallation(i.root, paths)
	if err != nil {
		return err
	}
	for _, artifact := range additions.artifacts {
		seen := false
		for _, existing := range i.artifacts {
			if Norm(existing.path) == Norm(artifact.path) {
				seen = true
				break
			}
		}
		if !seen {
			if len(i.artifacts) == 4096 {
				return errors.New("Installation artifact budget exceeded")
			}
			i.artifacts = append(i.artifacts, artifact)
		}
	}
	return nil
}

func (i *InstallationInstance) ScannedIdentity(path string) (*TargetIdentity, error) {
	for _, a := range i.artifacts {
		if Norm(a.path) != Norm(path) {
			continue
		}
		if _, err := os.Lstat(path); os.IsNotExist(err) {
			return nil, nil
		}
		if !a.unchanged() {
			return nil, errors.New("Scanned installation artifact changed or is unconfirmed")
		}
		id := *a.identity
		return &id, nil
	}
	return nil, errors.New("Path is outside the scanned installation evidence")
}
func (i *InstallationInstance) Explanation() map[string]any {
	items := make([]map[string]any, len(i.artifacts))
	for n, a := range i.artifacts {
		items[n] = map[string]any{"path": a.path, "identity_captured": a.identity != nil, "absent_at_scan": a.identity == nil}
	}
	return map[string]any{"root": i.root, "artifacts": items}
}

type OfficialUninstaller struct {
	Provider           string
	Executable         string
	Arguments          []string
	WorkingDirectory   string
	InstalledArtifacts []string
}
type OfficialOperation struct {
	uninstaller      OfficialUninstaller
	evidence         []installationArtifact
	workingDirectory *InstallationInstance
}

func CaptureOfficialOperation(command OfficialUninstaller, evidence []string) (*OfficialOperation, error) {
	md, err := os.Lstat(command.WorkingDirectory)
	if err != nil || !md.IsDir() || ruleEvidenceLink(md) {
		return nil, errors.New("Official working directory is missing or redirected")
	}
	op := &OfficialOperation{uninstaller: command}
	op.uninstaller.Arguments = append([]string(nil), command.Arguments...)
	op.uninstaller.InstalledArtifacts = append([]string(nil), command.InstalledArtifacts...)
	for _, path := range append([]string{command.Executable}, evidence...) {
		md, err := os.Lstat(path)
		if err != nil || !md.Mode().IsRegular() || ruleEvidenceLink(md) {
			return nil, errors.New("Official operation evidence is missing, redirected or not a file")
		}
		a, err := captureInstallationArtifact(path)
		if err != nil {
			return nil, err
		}
		op.evidence = append(op.evidence, a)
	}
	op.workingDirectory, err = CaptureInstallation(command.WorkingDirectory, []string{command.WorkingDirectory})
	if err != nil {
		return nil, err
	}
	return op, nil
}
func (o *OfficialOperation) Command() (OfficialUninstaller, error) {
	id, err := o.workingDirectory.ScannedIdentity(o.uninstaller.WorkingDirectory)
	if err != nil {
		return OfficialUninstaller{}, err
	}
	if id == nil {
		return OfficialUninstaller{}, errors.New("Official working directory disappeared since scan")
	}
	for _, a := range o.evidence {
		if !a.unchanged() {
			return OfficialUninstaller{}, errors.New("Official operation evidence changed or unavailable since scan")
		}
	}
	cmd := o.uninstaller
	cmd.Arguments = append([]string(nil), cmd.Arguments...)
	cmd.InstalledArtifacts = append([]string(nil), cmd.InstalledArtifacts...)
	return cmd, nil
}
func (o *OfficialOperation) Explanation() map[string]any {
	paths := make([]string, len(o.evidence))
	for i, a := range o.evidence {
		paths[i] = a.path
	}
	return map[string]any{"provider": o.uninstaller.Provider, "executable": o.uninstaller.Executable, "arguments": len(o.uninstaller.Arguments), "working_directory": o.uninstaller.WorkingDirectory, "evidence": paths}
}
