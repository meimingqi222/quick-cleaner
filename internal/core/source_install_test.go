package core

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"
)

type sourceFixture struct {
	home, code, state, roaming string
	rule                       SourceInstallRule
}

func sourceFixtureFile(t *testing.T, path, text string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}

func sourceTestCanonical(path string) (string, bool) {
	resolved, err := filepath.EvalSymlinks(path)
	return resolved, err == nil
}

func newSourceFixture(t *testing.T) sourceFixture {
	t.Helper()
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	f := sourceFixture{home: t.TempDir(), roaming: t.TempDir(), rule: clonePlanValue(*snapshot.Definition("hermes").App)}
	f.home, _ = sourceTestCanonical(f.home)
	f.code = filepath.Join(f.home, "hermes-agent")
	f.state = filepath.Join(f.home, "installs", sourceInstallKey(f.code))
	environment := filepath.Join(f.state, "environments", "generation", "venv")
	sourceFixtureFile(t, filepath.Join(environment, "pyvenv.cfg"), "runtime")
	facts, _ := json.Marshal(map[string]any{"packages": map[string]any{"venv": map[string]any{"environment": environment}}})
	sourceFixtureFile(t, filepath.Join(f.state, "facts.json"), string(facts))
	sourceFixtureFile(t, filepath.Join(f.code, "pyproject.toml"), `name = "hermes-agent"`)
	sourceFixtureFile(t, filepath.Join(f.home, "tools", "python-verified", "python.exe"), "runtime")
	sourceFixtureFile(t, filepath.Join(f.home, "tools", "facts.json"), `{"packages":{"python":{"entry":"python-verified","digest":"`+strings.Repeat("a", 64)+`"}}}`)
	sourceFixtureFile(t, filepath.Join(f.home, "tools", "untracked.txt"), "unowned")
	sourceFixtureFile(t, filepath.Join(f.home, "sessions", "keep.txt"), "user data")
	return f
}

func (f sourceFixture) build(owned bool) (*sourceInstallPlan, error) {
	return buildSourceInstallPlan(f.code, owned, f.rule, sourceTestCanonical, f.roaming)
}

func TestSourceInstallPlanningPreservesNarrowAuthority(t *testing.T) {
	f := newSourceFixture(t)
	sourceFixtureFile(t, filepath.Join(f.home, "hermes-setup.exe"), "MZ hermes_bootstrap_lib")
	sourceFixtureFile(t, filepath.Join(f.home, "bootstrap-cache", "install-main.ps1"), "# hermes-agent installer")
	sourceFixtureFile(t, filepath.Join(f.home, "bootstrap-cache", "install-other.ps1"), "# unrelated")
	sourceFixtureFile(t, filepath.Join(f.home, "gateway-service", "Hermes_Gateway.cmd"), filepath.Join(f.home, "hermes-agent", "start.py"))
	startup := filepath.Join(f.roaming, filepath.FromSlash("Microsoft/Windows/Start Menu/Programs/Startup/Hermes_Gateway.cmd"))
	sourceFixtureFile(t, startup, filepath.Join(f.home, "gateway-service", "run.cmd"))
	p, err := f.build(true)
	if err != nil {
		t.Fatal(err)
	}
	for _, relative := range []string{"hermes-agent", "tools/python-verified", "tools/facts.json", "tools/.install.lock", "bin/hermes.exe", "hermes-setup.exe", "bootstrap-cache/install-main.ps1", "gateway-service"} {
		if !slices.Contains(p.paths, filepath.Join(f.home, filepath.FromSlash(relative))) {
			t.Fatalf("missing owned path %s: %v", relative, p.paths)
		}
	}
	for _, relative := range []string{"tools", "tools/untracked.txt", "sessions", "sessions/keep.txt", "bootstrap-cache", "bootstrap-cache/install-other.ps1", "bin"} {
		if slices.Contains(p.paths, filepath.Join(f.home, filepath.FromSlash(relative))) {
			t.Fatalf("overbroad authority: %s", relative)
		}
	}
	if p.state != f.state || p.sharedTools || len(p.startup) != 1 || !slices.Contains(p.observedPaths(), startup) {
		t.Fatalf("state/startup: %+v", p)
	}
	if err := p.recheckShared(); err != nil {
		t.Fatal(err)
	}
	sourceFixtureFile(t, filepath.Join(f.home, "installs", "new-owner", "facts.json"), "{}")
	if err := p.recheckShared(); err == nil {
		t.Fatal("late shared owner was not detected")
	}
	shared, err := f.build(true)
	if err != nil {
		t.Fatal(err)
	}
	if !shared.sharedTools || len(shared.startup) != 0 {
		t.Fatal("shared install still owns startup/tools")
	}
	for _, path := range shared.paths {
		if atOrUnder(Norm(path), Norm(filepath.Join(f.home, "tools"))) || path == filepath.Join(f.home, "bin", "hermes.exe") {
			t.Fatalf("shared artifact claimed: %s", path)
		}
	}
	sourceFixtureFile(t, filepath.Join(f.home, "profiles", "other", "config.yaml"), "shared")
	if _, err := f.build(false); err == nil {
		t.Fatal("named profile allowed checkout removal")
	}
}

func TestSourceInstallRecoveryRequiresExactReceipt(t *testing.T) {
	f := newSourceFixture(t)
	if err := os.RemoveAll(f.code); err != nil {
		t.Fatal(err)
	}
	if _, err := f.build(false); err != nil {
		t.Fatalf("exact receipt should support recovery: %v", err)
	}
	other := filepath.Join(f.home, "unrelated", "hermes-agent")
	if err := os.MkdirAll(filepath.Dir(other), 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := buildSourceInstallPlan(other, false, f.rule, sourceTestCanonical, f.roaming); err == nil {
		t.Fatal("name alone authorized recovery")
	}
	for _, environment := range []string{filepath.Join(f.home, "sessions", "venv"), filepath.Join(f.state, "environments", "wrong-name")} {
		facts, _ := json.Marshal(map[string]any{"packages": map[string]any{"venv": map[string]any{"environment": environment}}})
		sourceFixtureFile(t, filepath.Join(f.state, "facts.json"), string(facts))
		if _, err := f.build(false); err == nil {
			t.Fatalf("foreign environment accepted: %s", environment)
		}
	}
	if got := sourceInstallKey(`C:\Fixture\Hermes\hermes-agent`); got != "346d26a042e42ce9" {
		t.Fatalf("receipt key=%s", got)
	}
}

func TestSourceInstallRejectsRedirectsCorruptFactsAndPreservedOverlap(t *testing.T) {
	for _, scenario := range []string{"invalid-json", "invalid-utf8", "bad-tool-digest", "tool-escape", "preserved-overlap", "linked-tool"} {
		t.Run(scenario, func(t *testing.T) {
			f := newSourceFixture(t)
			switch scenario {
			case "invalid-json":
				sourceFixtureFile(t, filepath.Join(f.state, "facts.json"), "{")
			case "invalid-utf8":
				sourceFixtureFile(t, filepath.Join(f.state, "facts.json"), "{\"x\":\"\xff\"}")
			case "bad-tool-digest":
				sourceFixtureFile(t, filepath.Join(f.home, "tools", "facts.json"), `{"packages":{"python":{"entry":"python-verified","digest":"bad"}}}`)
			case "tool-escape":
				sourceFixtureFile(t, filepath.Join(f.home, "tools", "facts.json"), `{"packages":{"python":{"entry":"python-../foreign","digest":"`+strings.Repeat("a", 64)+`"}}}`)
			case "preserved-overlap":
				f.rule.Launchers = append(f.rule.Launchers, "sessions")
			case "linked-tool":
				path := filepath.Join(f.home, "tools", "python-verified")
				if err := os.RemoveAll(path); err != nil {
					t.Fatal(err)
				}
				if err := os.Symlink(t.TempDir(), path); err != nil {
					t.Skipf("symlink unavailable: %v", err)
				}
			}
			if _, err := f.build(true); err == nil {
				t.Fatal("unsafe plan accepted")
			}
		})
	}
}

func (f sourceFixture) scanned(t *testing.T, p *sourceInstallPlan) *CleanupPlan {
	t.Helper()
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	plan := NewCleanupPlan(NewRuleRef(snapshot, "hermes", f.home), []PlannedTarget{{Path: f.code, Operation: Operation{Kind: OpOfficialUninstall}, Disposal: Permanent}})
	plan.installation, err = CaptureInstallation(f.code, p.observedPaths())
	if err != nil {
		t.Fatal(err)
	}
	return plan
}

func TestSourceInstallLifecycleKeepsRecoveryUntilConfirmed(t *testing.T) {
	for _, scenario := range []string{"success", "official-failure", "late-owner", "registration-failure", "process-resurrection", "nested-receipt", "empty-state", "startup-replaced", "shortcut-replaced", "shared"} {
		t.Run(scenario, func(t *testing.T) {
			f := newSourceFixture(t)
			startup := filepath.Join(f.roaming, filepath.FromSlash("Microsoft/Windows/Start Menu/Programs/Startup/Hermes_Gateway.cmd"))
			shortcut := filepath.Join(f.roaming, "Hermes.lnk")
			if scenario == "startup-replaced" {
				sourceFixtureFile(t, startup, filepath.Join(f.home, "gateway-service", "run.cmd"))
			}
			if scenario == "shortcut-replaced" {
				sourceFixtureFile(t, shortcut, "original")
			}
			if scenario == "shared" {
				sourceFixtureFile(t, filepath.Join(f.home, "installs", "another", "facts.json"), "{}")
			}
			if scenario == "nested-receipt" {
				f.rule.StateFacts = "metadata/nested/facts.json"
				data, err := os.ReadFile(filepath.Join(f.state, "facts.json"))
				if err != nil {
					t.Fatal(err)
				}
				sourceFixtureFile(t, filepath.Join(f.state, f.rule.StateFacts), string(data))
			}
			p, err := f.build(true)
			if err != nil {
				t.Fatal(err)
			}
			plan := f.scanned(t, p)
			var shortcuts []installationArtifact
			if scenario == "shortcut-replaced" {
				if err := plan.installation.ObserveScanPaths([]string{shortcut}); err != nil {
					t.Fatal(err)
				}
				artifact, err := captureInstallationArtifact(shortcut)
				if err != nil {
					t.Fatal(err)
				}
				shortcuts = append(shortcuts, artifact)
			}
			elapsed := time.Duration(0)
			start := time.Unix(1000, 0)
			r := sourceInstallRuntime{idle: func() error { return nil }, official: func() error { return nil }, registrations: func() error { return nil }, now: func() time.Time { return start.Add(elapsed) }, sleep: func(d time.Duration) { elapsed += d }}
			r.clean = func(items []ArbitraryTarget) CleanReport {
				spot := make(map[string]SpotCheck, len(items))
				for _, item := range items {
					spot[item.Path] = SpotClear
				}
				return cleanArbitraryItems(items, Permanent, NewCleanProgress(0, 0), spot)
			}
			r.official = func() error {
				if scenario == "startup-replaced" {
					if err := os.Rename(startup, startup+".old"); err != nil {
						t.Fatal(err)
					}
					sourceFixtureFile(t, startup, "replacement")
				}
				if scenario == "official-failure" {
					return errors.New("official command failed")
				}
				if scenario == "late-owner" {
					sourceFixtureFile(t, filepath.Join(f.home, "installs", "second-owner", "facts.json"), "{}")
				}
				if scenario == "empty-state" {
					if err := os.RemoveAll(filepath.Join(f.state, "environments")); err != nil {
						t.Fatal(err)
					}
					if err := os.Remove(filepath.Join(f.state, "facts.json")); err != nil {
						t.Fatal(err)
					}
				}
				return nil
			}
			r.registrations = func() error {
				if scenario == "shortcut-replaced" {
					if err := os.Rename(shortcut, shortcut+".old"); err != nil {
						t.Fatal(err)
					}
					sourceFixtureFile(t, shortcut, "replacement")
				}
				if _, err := os.Stat(filepath.Join(f.home, "tools", "facts.json")); err != nil {
					t.Fatal("tool recovery removed before verification", err)
				}
				if scenario != "empty-state" {
					if _, err := os.Stat(filepath.Join(f.state, f.rule.StateFacts)); err != nil {
						t.Fatal("installation recovery removed before verification", err)
					}
				}
				if scenario == "registration-failure" {
					return errors.New("registration unconfirmed")
				}
				return nil
			}
			if scenario == "process-resurrection" {
				r.idle = func() error {
					if elapsed >= time.Second {
						return errors.New("new application process")
					}
					return nil
				}
			}
			command := OfficialUninstaller{Provider: "fixture", InstalledArtifacts: append([]string(nil), p.paths...)}
			out := p.execute(plan, shortcuts, "", r, &command, 3*time.Second)
			wantOK := scenario == "success" || scenario == "nested-receipt" || scenario == "empty-state" || scenario == "shared"
			if (out.Result == nil) != wantOK {
				t.Fatalf("result=%v steps=%+v", out.Result, out.PlanExecutions)
			}
			if wantOK {
				if !sourceArtifactsRemoved(p.paths) {
					t.Fatal("owned artifacts remain")
				}
			} else {
				for _, path := range []string{filepath.Join(f.state, f.rule.StateFacts), filepath.Join(f.home, "tools", "facts.json")} {
					if _, err := os.Stat(path); err != nil {
						t.Fatal("failed lifecycle lost recovery", path, err)
					}
				}
			}
			for _, path := range []string{filepath.Join(f.home, "sessions", "keep.txt"), filepath.Join(f.home, "tools", "untracked.txt")} {
				if _, err := os.Stat(path); err != nil {
					t.Fatal("unowned data removed", path, err)
				}
			}
			if scenario == "shared" {
				if _, err := os.Stat(filepath.Join(f.home, "tools", "python-verified", "python.exe")); err != nil {
					t.Fatal("shared runtime removed", err)
				}
			}
			changed := ""
			if scenario == "startup-replaced" {
				changed = startup
			}
			if scenario == "shortcut-replaced" {
				changed = shortcut
			}
			if changed != "" {
				if data, err := os.ReadFile(changed); err != nil || string(data) != "replacement" {
					t.Fatal("replacement removed or modified", changed, err)
				}
			}
		})
	}
}

func TestSourceInstallExecutorCannotDeleteGenericTargets(t *testing.T) {
	path := filepath.Join(t.TempDir(), "keep.txt")
	sourceFixtureFile(t, path, "keep")
	plan := NewCleanupPlan(EngineRuleRef(), []PlannedTarget{{Path: path, Operation: Operation{Kind: OpOfficialUninstall}, Disposal: Permanent}})
	report := ExecuteSourcePlan(plan, 0, NewCleanProgress(0, 0), func(SourceAction) error { t.Fatal("generic target entered source lifecycle"); return nil })
	if ExecutionResult(report) == nil {
		t.Fatal("generic engine plan accepted by source capability")
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatal("generic target removed", err)
	}
}
