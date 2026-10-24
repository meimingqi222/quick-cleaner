package core

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func inspectSourceApp(target string, snapshot *RuleSnapshot, canonical func(string) (string, bool), roaming, powershell string) *InstalledApp {
	for _, definition := range snapshot.Bundle.Rules {
		if definition.App == nil {
			continue
		}
		rule := *definition.App
		home := target
		matched := false
		for range 12 {
			if slices.ContainsFunc(rule.Targets, func(relative string) bool {
				return Norm(filepath.Join(home, filepath.FromSlash(relative))) == Norm(target)
			}) {
				matched = true
				break
			}
			parent := filepath.Dir(home)
			if parent == home {
				break
			}
			home = parent
		}
		if !matched {
			continue
		}
		ref := NewRuleRef(snapshot, definition.ID, home)
		if ref.Revalidate() != nil {
			continue
		}
		code, ok := canonicalSourceTarget(filepath.Join(home, rule.Source), canonical)
		if !ok {
			continue
		}
		home, ok = canonical(home)
		if !ok || IsProtectedResidualPath(code) || IsProtectedResidualPath(home) {
			continue
		}
		projectPath := filepath.Join(code, rule.SourceFile)
		project, err := sourceReadFile(projectPath, 4*1024*1024)
		signature := err == nil && utf8.Valid(project) && sourceOwnedPath(code, projectPath) && slices.ContainsFunc(strings.Split(string(project), "\n"), func(line string) bool { return strings.TrimSpace(line) == rule.SourceSignature })
		constants, err := os.Stat(filepath.Join(code, rule.Constants))
		signature = signature && err == nil && constants.Mode().IsRegular()
		owner, err := os.Lstat(filepath.Join(code, rule.SourceOwner))
		owned := signature && err == nil && !ruleEvidenceLink(owner)
		var installation *sourceInstallPlan
		if !signature || owned {
			installation, _ = buildSourceInstallPlan(code, owned, rule, canonical, roaming)
		}
		if !signature && installation == nil {
			continue
		}
		app := &InstalledApp{Name: rule.Name, RegistryRoot: AppUnregistered}
		if stamp, err := sourceReadJSON(filepath.Join(code, rule.Stamp)); err == nil {
			if value, ok := jsonPointer(stamp, rule.VersionPointer); ok {
				app.Version, _ = value.(string)
			}
		}
		location := target
		discovery := &AppDiscovery{Executable: target, Rule: &ref, ProgramPaths: []string{target}}
		if installation != nil {
			location = code
			discovery.ProgramPaths = append([]string(nil), installation.paths...)
			plan := NewCleanupPlan(ref, []PlannedTarget{{Path: code, Operation: Operation{Kind: OpOfficialUninstall}, Disposal: Permanent}})
			plan.installation, err = CaptureInstallation(code, installation.observedPaths())
			if err != nil {
				continue
			}
			discovery.Plan = plan
			command := OfficialUninstaller{Provider: rule.Name + " recovery", Executable: powershell, Arguments: []string{"-NoProfile", "-NonInteractive", "-Command", "exit 0"}, WorkingDirectory: home, InstalledArtifacts: append([]string(nil), installation.paths...)}
			var evidence []string
			module := filepath.Join(code, filepath.FromSlash(rule.ModuleFile))
			if _, err := sourceReadFile(module, 4*1024*1024); err == nil {
				if python := sourceExternalPython(home, code, rule, canonical); python != "" {
					command.Provider, command.Executable = rule.Name, python
					command.Arguments = []string{"-I", "-c", sourcePythonLauncher, code, home, rule.HomeVariable, rule.Module, rule.ModuleFile}
					command.Arguments = append(command.Arguments, rule.ModuleArgs...)
					evidence = []string{module}
				}
			}
			if frozen, captureErr := CaptureOfficialOperation(command, evidence); captureErr == nil {
				plan.official = frozen
				discovery.Uninstaller = &command
			}
		}
		app.ID = "discovered:" + Norm(location)
		app.InstallLocation = &location
		app.DisplayIcon = &target
		app.Discovery = discovery
		return app
	}
	return nil
}

const sourcePythonLauncher = "import os,runpy,sys;root,home,variable,module,relative=sys.argv[1:6];args=sys.argv[6:];sys.path.insert(0,root);os.environ[variable]=home;os.environ.pop('PYTHONPATH',None);sys.argv=[module]+args;os.path.isfile(os.path.join(root,relative)) and runpy.run_module(module,run_name='__main__')"

func sourceExternalPython(home, code string, rule SourceInstallRule, canonical func(string) (string, bool)) string {
	f, err := os.Open(filepath.Join(home, rule.ToolsDir))
	if err != nil {
		return ""
	}
	defer f.Close()
	entries, _ := f.ReadDir(100)
	var paths []string
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), rule.PythonPrefix) {
			paths = append(paths, filepath.Join(home, rule.ToolsDir, entry.Name(), "python.exe"))
		}
	}
	slices.Sort(paths)
	for _, path := range paths {
		path, ok := canonical(path)
		if !ok || atOrUnder(Norm(path), Norm(code)) || IsProtectedResidualPath(path) {
			continue
		}
		if info, err := os.Stat(path); err == nil && info.Mode().IsRegular() {
			return path
		}
	}
	return ""
}

func sourceInstallIdle(home string, rule SourceInstallRule, processes []platform.AppRunningProcess, err error) error {
	if err != nil {
		return err
	}
	for _, process := range processes {
		if strings.EqualFold(process.Name, rule.InstallerProcess) || process.Image != "" && atOrUnder(Norm(process.Image), Norm(home)) || process.Image == "" && slices.ContainsFunc(rule.UnknownProcesses, func(name string) bool { return strings.EqualFold(name, process.Name) }) {
			return fmt.Errorf("installation/application still running: %s (PID %d)", process.Name, process.PID)
		}
	}
	return nil
}
