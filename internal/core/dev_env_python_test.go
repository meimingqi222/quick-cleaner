package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestDevPythonProbeAndScopedInterpreters(t *testing.T) {
	bin := t.TempDir()
	for _, name := range []string{"python", "python3"} {
		if runtime.GOOS == "windows" {
			name += ".exe"
		}
		p := filepath.Join(bin, name)
		devFixtureWrite(t, p, "")
		if e := os.Chmod(p, 0755); e != nil {
			t.Fatal(e)
		}
	}
	t.Setenv("PATH", bin)
	install, user := filepath.Join(t.TempDir(), "install"), filepath.Join(t.TempDir(), "user")
	for _, p := range []string{filepath.Join(install, "pip.dist-info"), filepath.Join(user, "a.dist-info"), filepath.Join(user, "b.dist-info")} {
		if e := os.MkdirAll(p, 0755); e != nil {
			t.Fatal(e)
		}
	}
	run := DevToolRunner(func(_ string, args []string, _ time.Duration) (ProcRun, bool) {
		if len(args) != 2 || args[0] != "-c" {
			t.Fatal(args)
		}
		return ProcRun{Stdout: []byte(devFixtureJSON(map[string]any{"prefix": "same-prefix", "version": "3.12", "install": install, "user": user})), OK: true}, true
	})
	items := PythonInterpreters(run)
	if len(items) != 2 || items[0].Kind.Scope != SiteInstall || items[1].Kind.Scope != SiteUser || *items[0].Kind.PackageCount != 1 || *items[1].Kind.PackageCount != 2 {
		t.Fatal(items)
	}
	for _, i := range items {
		if i.Source != AssetTool || i.LastChange == nil || i.LastChange.Source != TimeSitePackages || i.Kind.Python == "" {
			t.Fatal(i)
		}
	}
	facts := ProbePython(func(string, []string, time.Duration) (ProcRun, bool) {
		return ProcRun{Stdout: []byte(`{"install":["wrong"],"user":""}`), OK: true}, true
	}, "python")
	if facts.InstallSite != nil || facts.UserSite != nil {
		t.Fatal(facts)
	}
}

func TestDevVirtualEnvMetadataAndIdentity(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	venv := filepath.Join(root, "project")
	site := filepath.Join(venv, "lib", "python3.12", "site-packages")
	devFixtureWrite(t, filepath.Join(venv, "pyvenv.cfg"), "home = /base python\nVERSION = 3.12.1\n")
	devFixtureWrite(t, filepath.Join(site, "pkg.dist-info", "METADATA"), "Name: pkg\nVersion: 1\n")
	if e := os.MkdirAll(filepath.Join(root, "not-a-venv"), 0755); e != nil {
		t.Fatal(e)
	}
	items := VirtualEnvs([]VenvRoot{{root, VenvVirtualenvwrapper}}, []string{venv})
	if len(items) != 1 {
		t.Fatal(items)
	}
	i := items[0]
	if i.Source != AssetTool || i.Kind.Identity == nil || !i.Kind.Identity.Recheck(venv) || i.Kind.Version == nil || *i.Kind.Version != "3.12.1" || *i.Kind.BasePython != "/base python" || *i.Kind.PackageCount != 1 || i.Kind.VenvManager != VenvVirtualenvwrapper || i.LastChange.Source != TimeSitePackages {
		t.Fatal(i)
	}
	if _, ok := VenvPython(venv); ok {
		t.Fatal("missing interpreter")
	}
	python := filepath.Join(venv, "bin", "python")
	if runtime.GOOS == "windows" {
		python = filepath.Join(venv, "Scripts", "python.exe")
	}
	devFixtureWrite(t, python, "")
	if got, ok := VenvPython(venv); !ok || got != python {
		t.Fatalf("%s %v", got, ok)
	}
	devFixtureWrite(t, filepath.Join(venv, "pyvenv.cfg"), "version = 3.12\ninvalid = \xff")
	if got := VirtualEnvs(nil, []string{venv}); len(got) != 0 {
		t.Fatal("invalid UTF-8 cfg accepted", got)
	}
}

func TestDevPipPackagesTrustMetadataHeadersOnly(t *testing.T) {
	site := t.TempDir()
	for dir, text := range map[string]string{"misleading-1.dist-info": "nAmE: python-dateutil\r\nVersion: 2.9\r\n\r\nName: forged\n", "bad.dist-info": "Name: bad & command\n", "body.dist-info": "Metadata-Version: 2\n\nName: hidden\n", "empty.dist-info": "Name: \n", "invalid-utf8.dist-info": "Name: valid-name\n\ninvalid \xff", "versionless.dist-info": "Name: good_name\n"} {
		devFixtureWrite(t, filepath.Join(site, dir, "METADATA"), text)
	}
	items := PipPackages(site, "/exact/python")
	if len(items) != 2 || items[0].DisplayName() != "good_name" || items[0].Kind.Version != nil || items[1].DisplayName() != "python-dateutil" || *items[1].Kind.Version != "2.9" {
		t.Fatal(items)
	}
	for _, i := range items {
		if i.Kind.Python != "/exact/python" || i.Source != AssetTool || i.LastChange == nil || i.LastChange.Source != TimePackageManifest || i.Size.LogicalBytes != 0 {
			t.Fatal(i)
		}
	}
	for _, name := range []string{"", "space name", "quote\"", "é", "a;cmd"} {
		if IsValidDistributionName(name) {
			t.Fatal(name)
		}
	}
	if _, ok := MetadataField("Verſion: forged\n", "Version"); ok {
		t.Fatal("non-ASCII header alias accepted")
	}
}

func TestDevPoetryPathsAndUnknownInterpreterFacts(t *testing.T) {
	bin := t.TempDir()
	for _, name := range []string{"python", "poetry"} {
		if runtime.GOOS == "windows" {
			name += ".exe"
		}
		path := filepath.Join(bin, name)
		devFixtureWrite(t, path, "")
		if e := os.Chmod(path, 0755); e != nil {
			t.Fatal(e)
		}
	}
	t.Setenv("PATH", bin)
	items := PythonInterpreters(devFixtureRun(nil))
	if len(items) != 1 || items[0].Kind.PackageCount != nil || items[0].Kind.Version != nil || items[0].Source != AssetLayout || items[0].LastChange != nil {
		t.Fatal(items)
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "project-venv")
	devFixtureWrite(t, filepath.Join(path, "pyvenv.cfg"), "version = 3.11\n")
	run := devFixtureRun(map[string]ProcRun{"poetry env list --full-path": {Stdout: []byte("relative\n  " + path + "\n"), OK: true}})
	paths := PoetryEnvPaths(run)
	if len(paths) != 1 || paths[0] != path {
		t.Fatal(paths)
	}
	items = VirtualEnvs(nil, paths)
	if len(items) != 1 || items[0].Kind.VenvManager != VenvPoetry || items[0].Kind.PackageCount != nil || items[0].LastChange != nil {
		t.Fatal(items)
	}
}
