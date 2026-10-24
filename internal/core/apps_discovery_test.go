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

// Qoder 的开始菜单快捷方式指向 %LOCALAPPDATA%\Qoder\Qoder Launcher\Qoder Launcher.exe，
// 不在登记安装目录 %LOCALAPPDATA%\Programs\Qoder 之下，旧逻辑会把它再列成一个未登记应用。
func TestAppDiscoveryVendorLauncherBelongsToRegisteredApp(t *testing.T) {
	root := t.TempDir()
	write := func(parts ...string) string {
		path := filepath.Join(append([]string{root}, parts...)...)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("fixture"), 0600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	install, installCN := filepath.Join(root, "Programs", "Qoder"), filepath.Join(root, "Programs", "Qoder CN")
	write("Programs", "Qoder", "Qoder.exe")
	write("Programs", "Qoder CN", "Qoder CN.exe")
	launcher := write("Qoder", "Qoder Launcher", "Qoder Launcher.exe")
	launcherCN := write("Qoder CN", "Qoder CN Launcher", "Qoder CN Launcher.exe")
	registered := []InstalledApp{{Name: "Qoder 0.4.11", InstallLocation: &install}, {Name: "Qoder CN 0.4.4", InstallLocation: &installCN}}
	canonical := func(path string) (string, bool) { return path, true }
	candidates := []platform.ShortcutAppEvidence{
		{Shortcut: filepath.Join(root, "Start Menu", "Qoder.lnk"), Target: launcher},
		{Shortcut: filepath.Join(root, "Desktop", "Qoder CN.lnk"), Target: launcherCN},
	}
	if got := discoverPortableApps(context.Background(), registered, candidates, canonical); len(got) != 0 {
		t.Fatalf("vendor launcher listed as a separate app: %+v", got)
	}
	// 只登记了国际版时，国内版的 launcher 不能被国际版认领。
	if got := discoverPortableApps(context.Background(), registered[:1], candidates, canonical); len(got) != 1 || got[0].Discovery.Executable != launcherCN {
		t.Fatalf("launcher claimed across editions: %+v", got)
	}
	// 同名但快捷方式名不同、或路径里没有与安装目录同名的层级，仍是独立应用。
	for _, c := range []platform.ShortcutAppEvidence{
		{Shortcut: filepath.Join(root, "Desktop", "Qoder Nightly.lnk"), Target: launcher},
		{Shortcut: filepath.Join(root, "Desktop", "Qoder.lnk"), Target: write("Other", "Launcher", "Qoder.exe")},
	} {
		if got := discoverPortableApps(context.Background(), registered, []platform.ShortcutAppEvidence{c}, canonical); len(got) != 1 {
			t.Fatalf("weak evidence suppressed %+v: %+v", c, got)
		}
	}
}
