package core

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestDiscoveredResidualAuthority(t *testing.T) {
	f := newSourceFixture(t)
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	inspect := func(target string, s *RuleSnapshot) *InstalledApp {
		return inspectSourceApp(target, s, sourceTestCanonical, f.roaming, "powershell.exe")
	}
	target := filepath.Join(f.home, filepath.FromSlash(f.rule.Targets[0]))
	app := inspect(target, snapshot)
	if app == nil {
		t.Fatal("fixture not discovered")
	}
	idle := func() ([]platform.AppRunningProcess, error) { return nil, nil }
	item := func(path string) ResidualItem {
		i := CertainResidual(ResidualKind{Type: ResidualDirectory, Path: path}, ResidualInstallDir)
		i.Rule = app.Discovery.Rule
		return i
	}
	check := func(items []ResidualItem, processes func() ([]platform.AppRunningProcess, error)) error {
		return validateDiscoveredResidualClean(context.Background(), *app, items, inspect, processes)
	}
	tools := item(filepath.Join(f.home, "tools", "python-verified"))
	if err := check([]ResidualItem{tools}, idle); err != nil {
		t.Fatalf("reviewed exact tool: %v", err)
	}
	for _, path := range []string{f.home, filepath.Join(f.home, "tools"), filepath.Join(f.home, "tools", "python-verified", "python.exe"), filepath.Join(f.home, "sessions")} {
		if err := check([]ResidualItem{item(path)}, idle); err == nil {
			t.Fatalf("unreviewed scope accepted: %s", path)
		}
	}
	other := &RuleSnapshot{Bundle: clonePlanValue(snapshot.Bundle)}
	mixed := item(f.code)
	ref := NewRuleRef(other, app.Discovery.Rule.id, f.home)
	mixed.Rule = &ref
	if err := check([]ResidualItem{tools, mixed}, idle); err == nil {
		t.Fatal("mixed generations accepted")
	}
	missing := item(f.code)
	missing.Rule = nil
	if err := check([]ResidualItem{missing}, idle); err == nil {
		t.Fatal("unknown rule accepted")
	}
	for _, process := range []platform.AppRunningProcess{
		{Name: f.rule.InstallerProcess},
		{Name: "runtime", Image: filepath.Join(f.home, "bin", "running.exe")},
		{Name: f.rule.UnknownProcesses[0]},
	} {
		busy := func() ([]platform.AppRunningProcess, error) { return []platform.AppRunningProcess{process}, nil }
		if err := check([]ResidualItem{tools}, busy); err == nil {
			t.Fatalf("live/unknown related owner accepted: %+v", process)
		}
	}
	if err := check([]ResidualItem{tools}, func() ([]platform.AppRunningProcess, error) { return nil, errors.New("unknown inventory") }); err == nil {
		t.Fatal("unknown process inventory accepted")
	}
	// A new installation owns shared tools now. The old exact review is no
	// longer authoritative, but the original code path remains independently owned.
	sourceFixtureFile(t, filepath.Join(f.home, "installs", "new-owner", "facts.json"), "{}")
	if err := check([]ResidualItem{tools}, idle); err == nil {
		t.Fatal("late shared owner accepted")
	}
	if err := check([]ResidualItem{item(f.code)}, idle); err != nil {
		t.Fatalf("ownership shrink incorrectly became global absence: %v", err)
	}
	sourceFixtureFile(t, filepath.Join(f.home, "profiles", "owner", "config.yaml"), "live owner")
	if err := check([]ResidualItem{item(f.code)}, idle); err == nil {
		t.Fatal("changed source ownership accepted")
	}
}

func TestDiscoveredResidualUnknownAndCancelled(t *testing.T) {
	inspect := func(string, *RuleSnapshot) *InstalledApp { t.Fatal("unexpected inspection"); return nil }
	processes := func() ([]platform.AppRunningProcess, error) { t.Fatal("unexpected process read"); return nil, nil }
	if err := validateDiscoveredResidualClean(context.Background(), InstalledApp{Discovery: &AppDiscovery{}}, nil, inspect, processes); err == nil {
		t.Fatal("portable evidence authorized a source installation")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := validateDiscoveredResidualClean(ctx, InstalledApp{}, nil, inspect, processes); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation: %v", err)
	}
}
