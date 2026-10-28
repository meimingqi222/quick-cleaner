package core

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestPortableResidualAuthority(t *testing.T) {
	for _, scenario := range []string{
		"exact", "shortcut only", "parent", "descendant", "foreign file", "directory", "data", "registry",
		"changed shortcut", "unknown shortcut", "foreign shortcut", "foreign owner", "replaced executable", "replaced shortcut", "missing identity",
		"live owner", "unknown owner", "unknown image", "unknown inventory", "unrelated neighbor", "same name elsewhere",
		"wrong app id", "mixed programs", "unknown programs", "foreign rule", "mixed snapshots", "missing rule", "cancelled",
	} {
		t.Run(scenario, func(t *testing.T) {
			root := t.TempDir()
			root, _ = sourceTestCanonical(root)
			exe, link, neighbor := filepath.Join(root, "portable.exe"), filepath.Join(root, "Portable.lnk"), filepath.Join(root, "neighbor.exe")
			for _, path := range []string{exe, link, neighbor} {
				sourceFixtureFile(t, path, "fixture")
			}
			snapshot, err := CurrentRules()
			if err != nil {
				t.Fatal(err)
			}
			ref := NewRuleRef(snapshot, "residual-windows", "")
			item := func(path string, source ResidualSource) ResidualItem {
				i := CertainResidual(ResidualKind{Type: ResidualFile, Path: path}, source)
				i.Rule = &ref
				return i
			}
			app := InstalledApp{ID: "discovered:" + Norm(exe), RegistryRoot: AppUnregistered, Discovery: &AppDiscovery{Executable: exe, ProgramPaths: []string{exe}, Shortcuts: []string{link}}}
			items := []ResidualItem{item(exe, ResidualInstallDir), item(link, ResidualShortcut)}
			target, readable := exe, true
			var owners []platform.AppRunningProcess
			var processErr error
			ctx := context.Background()
			switch scenario {
			case "shortcut only":
				items = items[1:]
			case "parent":
				items[0] = item(root, ResidualInstallDir)
			case "descendant":
				items[0].Kind.Path = filepath.Join(exe, "child")
			case "foreign file":
				items[0] = item(neighbor, ResidualInstallDir)
			case "directory":
				items[0].Kind.Type = ResidualDirectory
			case "data":
				items[0].Source = ResidualAppDataDir
			case "registry":
				items[0].Kind.Type = ResidualRegistryKey
			case "changed shortcut":
				target = neighbor
			case "unknown shortcut":
				readable = false
			case "foreign shortcut":
				foreign := filepath.Join(root, "Foreign.lnk")
				sourceFixtureFile(t, foreign, "fixture")
				items[1] = item(foreign, ResidualShortcut)
			case "foreign owner":
				items[0] = items[0].ForBundle("foreign.bundle")
			case "replaced executable", "replaced shortcut":
				path := exe
				if scenario == "replaced shortcut" {
					path = link
				}
				info, err := os.Stat(path)
				if err != nil {
					t.Fatal(err)
				}
				if err := os.Rename(path, path+".old"); err != nil {
					t.Fatal(err)
				}
				sourceFixtureFile(t, path, "fixture")
				if err := os.Chtimes(path, info.ModTime(), info.ModTime()); err != nil {
					t.Fatal(err)
				}
			case "missing identity":
				items[0].Identity = nil
			case "live owner":
				owners = []platform.AppRunningProcess{{Name: "renamed", Image: exe}}
			case "unknown owner":
				owners = []platform.AppRunningProcess{{Name: "PORTABLE.EXE"}}
			case "unknown image":
				owners = []platform.AppRunningProcess{{Name: "portable", Image: filepath.Join(root, "unreadable")}}
			case "unknown inventory":
				processErr = errors.New("inventory unavailable")
			case "unrelated neighbor":
				owners = []platform.AppRunningProcess{{Name: "neighbor.exe", Image: neighbor}, {Name: "unrelated.exe"}}
			case "same name elsewhere":
				owners = []platform.AppRunningProcess{{Name: "portable.exe", Image: neighbor}}
			case "wrong app id":
				app.ID = "discovered:" + Norm(root)
			case "mixed programs":
				app.Discovery.ProgramPaths = append(app.Discovery.ProgramPaths, neighbor)
			case "unknown programs":
				app.Discovery.ProgramPaths = nil
			case "foreign rule":
				other := EngineRuleRef()
				items[0].Rule = &other
			case "mixed snapshots":
				other := NewRuleRef(&RuleSnapshot{Bundle: clonePlanValue(snapshot.Bundle)}, "residual-windows", "")
				items[1].Rule = &other
			case "missing rule":
				items[0].Rule = nil
			case "cancelled":
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}
			err = validatePortableResidualClean(ctx, app, items, sourceTestCanonical,
				func(string) (string, bool) { return target, readable },
				func() ([]platform.AppRunningProcess, error) { return owners, processErr })
			want := scenario == "exact" || scenario == "shortcut only" || scenario == "unrelated neighbor" || scenario == "same name elsewhere"
			if (err == nil) != want {
				t.Fatalf("accepted=%v want=%v: %v", err == nil, want, err)
			}
		})
	}
}
