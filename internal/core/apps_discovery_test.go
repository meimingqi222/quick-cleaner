package core

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

func TestAppDiscoveryPortableEvidenceNeverOwnsParent(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "portable.exe")
	other := filepath.Join(root, "unrelated.txt")
	for _, path := range []string{target, other} {
		if err := os.WriteFile(path, []byte("fixture"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	first, second := filepath.Join(root, "Portable.lnk"), filepath.Join(root, "Alias.lnk")
	candidates := []platform.ShortcutAppEvidence{{Shortcut: first, Target: target}, {Shortcut: second, Target: target}, {Shortcut: first, Target: target}, {Target: other}, {Target: filepath.Join(root, "missing.exe")}}
	canonical := func(path string) (string, bool) { return path, true }
	apps := discoverPortableApps(context.Background(), nil, candidates, canonical)
	if len(apps) != 1 {
		t.Fatalf("apps=%+v", apps)
	}
	app := apps[0]
	if app.Discovery == nil || app.RegistryRoot != AppUnregistered || app.CanUninstall() || app.Discovery.Uninstaller != nil || app.Discovery.Plan != nil {
		t.Fatalf("launch evidence granted uninstall authority: %+v", app)
	}
	if app.InstallLocation == nil || *app.InstallLocation != target || !reflect.DeepEqual(app.Discovery.ProgramPaths, []string{target}) {
		t.Fatalf("parent claimed: %+v", app.Discovery)
	}
	if !reflect.DeepEqual(app.Discovery.Shortcuts, []string{second, first}) {
		t.Fatalf("shortcuts=%v", app.Discovery.Shortcuts)
	}
	registered := InstalledApp{InstallLocation: &root}
	if got := discoverPortableApps(context.Background(), []InstalledApp{registered}, candidates, canonical); len(got) != 0 {
		t.Fatalf("registered subtree rediscovered: %+v", got)
	}
	registered.InstallLocation = &other
	if got := discoverPortableApps(context.Background(), []InstalledApp{registered}, candidates, canonical); len(got) != 1 {
		t.Fatalf("neighbor path incorrectly covers executable: %+v", got)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if got := discoverPortableApps(ctx, nil, candidates, canonical); len(got) != 0 {
		t.Fatal("cancelled discovery returned apps")
	}
	if got := discoverPortableApps(context.Background(), nil, candidates, func(string) (string, bool) { return "", false }); len(got) != 0 {
		t.Fatal("unknown canonical identity accepted")
	}
}
