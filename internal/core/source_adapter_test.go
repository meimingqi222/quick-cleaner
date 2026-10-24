package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestSourceAppDiscoveryFreezesOfficialRouteAndInstance(t *testing.T) {
	for _, scenario := range []string{"source", "no-module", "no-python", "sealed", "recovery"} {
		t.Run(scenario, func(t *testing.T) {
			f := newSourceFixture(t)
			snapshot, err := CurrentRules()
			if err != nil {
				t.Fatal(err)
			}
			sourceFixtureFile(t, filepath.Join(f.code, f.rule.Constants), "fixture")
			sourceFixtureFile(t, filepath.Join(f.code, f.rule.SourceOwner, "HEAD"), "ref: refs/heads/main")
			sourceFixtureFile(t, filepath.Join(f.code, f.rule.ModuleFile), "# official module")
			sourceFixtureFile(t, filepath.Join(f.code, f.rule.Stamp), `{"displayVersion":"1.2.3"}`)
			powershell := filepath.Join(t.TempDir(), "powershell.exe")
			sourceFixtureFile(t, powershell, "fixture")
			if scenario == "no-module" {
				if err := os.Remove(filepath.Join(f.code, f.rule.ModuleFile)); err != nil {
					t.Fatal(err)
				}
			}
			if scenario == "no-python" {
				if err := os.Remove(filepath.Join(f.home, "tools", "python-verified", "python.exe")); err != nil {
					t.Fatal(err)
				}
			}
			if scenario == "sealed" {
				if err := os.RemoveAll(filepath.Join(f.code, f.rule.SourceOwner)); err != nil {
					t.Fatal(err)
				}
			}
			if scenario == "recovery" {
				if err := os.RemoveAll(f.code); err != nil {
					t.Fatal(err)
				}
			}
			inspect := func(target string) *InstalledApp {
				return inspectSourceApp(target, snapshot, sourceTestCanonical, f.roaming, powershell)
			}
			var candidates []platform.ShortcutAppEvidence
			for _, relative := range f.rule.Targets {
				candidates = append(candidates, platform.ShortcutAppEvidence{Target: filepath.Join(f.home, filepath.FromSlash(relative))})
			}
			apps := discoverAppCandidates(context.Background(), nil, candidates, sourceTestCanonical, inspect)
			if scenario == "sealed" {
				if len(apps) != len(f.rule.Targets) {
					t.Fatalf("sealed instances=%+v", apps)
				}
				for _, app := range apps {
					if app.Discovery.Plan != nil || app.Discovery.Uninstaller != nil || len(app.Discovery.ProgramPaths) != 1 {
						t.Fatal("sealed bundle authorized source removal")
					}
				}
				return
			}
			if len(apps) != 1 {
				t.Fatalf("apps=%+v", apps)
			}
			app := apps[0]
			if app.ID != "discovered:"+Norm(f.code) || app.Discovery.Plan == nil || app.Discovery.Uninstaller == nil {
				t.Fatalf("source/recovery discovery=%+v", app)
			}
			command, err := app.Discovery.Plan.official.Command()
			if err != nil {
				t.Fatal(err)
			}
			if scenario == "source" {
				if command.Executable != filepath.Join(f.home, "tools", "python-verified", "python.exe") || command.WorkingDirectory != f.home || !reflect.DeepEqual(command.Arguments[len(command.Arguments)-2:], []string{"--mode", "lite"}) {
					t.Fatalf("command=%+v", command)
				}
				module := filepath.Join(f.code, f.rule.ModuleFile)
				if err := os.Rename(module, module+".old"); err != nil {
					t.Fatal(err)
				}
				sourceFixtureFile(t, module, "replacement")
				if _, err := app.Discovery.Plan.official.Command(); err == nil {
					t.Fatal("replaced module remained authorized")
				}
			} else {
				if command.Executable != powershell || command.Arguments[len(command.Arguments)-1] != "exit 0" {
					t.Fatalf("recovery=%+v", command)
				}
				sourceFixtureFile(t, filepath.Join(f.code, f.rule.ModuleFile), "late module")
				sourceFixtureFile(t, filepath.Join(f.home, "tools", "python-verified", "python.exe"), "late runtime")
				after, err := app.Discovery.Plan.official.Command()
				if err != nil || !reflect.DeepEqual(command, after) {
					t.Fatal("new runtime redirected scanned command", after, err)
				}
			}
		})
	}
}

func TestSourceInstallIdleRequiresCompleteProcessEvidence(t *testing.T) {
	f := newSourceFixture(t)
	for _, tc := range []struct {
		name, image string
		busy        bool
	}{
		{"hermes-setup.exe", filepath.Join(t.TempDir(), "setup.exe"), true},
		{"other.exe", filepath.Join(f.home, "bin", "helper.exe"), true},
		{"python.exe", "", true},
		{"python.exe", filepath.Join(f.home+"-other", "python.exe"), false},
		{"unrelated.exe", "", false},
	} {
		err := sourceInstallIdle(f.home, f.rule, []platform.AppRunningProcess{{PID: 123, Name: tc.name, Image: tc.image}}, nil)
		if (err != nil) != tc.busy {
			t.Fatalf("%+v: %v", tc, err)
		}
	}
	if err := sourceInstallIdle(f.home, f.rule, nil, errors.New("snapshot failed")); err == nil {
		t.Fatal("failed enumeration authorized removal")
	}
}

func TestSourceRegistrationValuesRemainLiteralData(t *testing.T) {
	f := newSourceFixture(t)
	script := sourceRegistrationScript(`C:\Users\O'Brien\$(Invoke-Expression x)`, "FixtureEnvironment", false, false, f.rule)
	for _, want := range []string{`$installHome = 'C:\Users\O''Brien\$(Invoke-Expression x)'`, `$exclusive = $false`, `$checkTasks = $false`, `$environmentKey = 'FixtureEnvironment'`} {
		if !strings.Contains(script, want) {
			t.Fatalf("missing quoted option %q", want)
		}
	}
}
