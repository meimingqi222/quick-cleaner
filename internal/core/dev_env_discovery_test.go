package core

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestDevCondaMambaFallbackAndProtection(t *testing.T) {
	root := t.TempDir()
	env := filepath.Join(root, "envs", "work")
	devFixtureWrite(t, filepath.Join(env, "conda-meta", "python-3.11.8-build.json"), `{}`)
	devFixtureWrite(t, filepath.Join(env, "conda-meta", "history"), "transaction")
	t.Setenv("CONDA_PREFIX", env)
	run := devFixtureRun(map[string]ProcRun{"conda info --json": {Stdout: []byte("bad"), OK: true}, "mamba info --json": {Stdout: []byte(devFixtureJSON(map[string]any{"envs": []string{env, root, env}, "root_prefix": root})), OK: true}})
	items := CondaEnvironments(run)
	if len(items) != 2 || !items[0].Kind.IsBase || !items[0].IsProtected() || items[0].DisplayName() != "base" {
		t.Fatal(items)
	}
	i := items[1]
	if !i.IsActiveEnv || !i.IsProtected() || i.Source != AssetTool || i.Kind.PythonVersion == nil || *i.Kind.PythonVersion != "3.11.8" || i.Kind.PackageCount == nil || *i.Kind.PackageCount != 1 || i.LastChange == nil || i.LastChange.Source != TimeCondaTransaction {
		t.Fatalf("%+v", i)
	}
}

func TestDevCondaLayoutRegistryIsDisplayOnly(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("original desktop user home is cached by the platform; HOME cannot isolate it")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CACHE_HOME", filepath.Join(home, "cache"))
	base := filepath.Join(home, "miniconda3")
	env := filepath.Join(base, "envs", "work")
	other := filepath.Join(home, "elsewhere")
	for _, p := range []string{env, other} {
		if e := os.MkdirAll(p, 0755); e != nil {
			t.Fatal(e)
		}
	}
	devFixtureWrite(t, filepath.Join(home, ".conda", "environments.txt"), other+"\n"+env+"\n/missing\n")
	items := CondaEnvironments(devFixtureRun(nil))
	found := map[string]DevAssetItem{}
	for _, i := range items {
		found[i.Path] = i
		if i.Source != AssetLayout {
			t.Fatal(i)
		}
	}
	if len(found) != 3 || !found[base].Kind.IsBase || found[other].Path == "" || found[filepath.Join(home, ".conda")].Path != "" {
		t.Fatal(items)
	}
	empty := devFixtureRun(map[string]ProcRun{"conda info --json": {Stdout: []byte(`{"envs":[]}`), OK: true}})
	if got := CondaEnvironments(empty); len(got) != 0 {
		t.Fatal(got)
	}
}

func TestDevNodeTopLevelManifestAndFallback(t *testing.T) {
	root := filepath.Join(t.TempDir(), "node_modules")
	for name, manifest := range map[string]string{"top": `{"name":"top","version":"1","bin":{"cli":"bin.js"}}`, "transitive": `{"name":"transitive"}`, "@scope/tool": `{"name":"@scope/tool","bin":"bin/tool.js"}`, ".pnpm": `{}`} {
		devFixtureWrite(t, filepath.Join(root, name, "package.json"), manifest)
	}
	manifest := filepath.Join(filepath.Dir(root), "package.json")
	devFixtureWrite(t, manifest, `{"dependencies":{"top":"*","@scope/tool":"*"}}`)
	items := ScanNodeModules("pnpm", root)
	if len(items) != 2 {
		t.Fatal(items)
	}
	for _, i := range items {
		if i.Source != AssetTool || i.LastChange == nil || i.LastChange.Source != TimePackageManifest || len(i.Kind.BinShims) != 1 || i.DisplayName() == "transitive" {
			t.Fatal(i)
		}
	}
	devFixtureWrite(t, manifest, `{"dependencies":{}}`)
	if got := ScanNodeModules("pnpm", root); len(got) != 0 {
		t.Fatal(got)
	}
	for _, text := range []string{`{}`, `broken`} {
		devFixtureWrite(t, manifest, text)
		items = ScanNodeModules("pnpm", root)
		if len(items) != 3 {
			t.Fatal(items)
		}
		for _, i := range items {
			if i.Source != AssetLayout {
				t.Fatal(i)
			}
		}
	}
}

func TestDevPythonToolMetadataPrecedence(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "ruff")
	devFixtureWrite(t, filepath.Join(path, "uv-receipt.toml"), "receipt")
	run := devFixtureRun(map[string]ProcRun{"uv tool dir": {Stdout: []byte(root), OK: true}, "pipx list --json": {Stdout: []byte(`{"venvs":{}}`), OK: true}})
	items := PythonTools(run)
	if len(items) != 1 || items[0].Source != AssetTool || items[0].LastChange == nil || items[0].LastChange.Source != TimeToolMetadata || items[0].Kind.ToolKind != PythonToolUv {
		t.Fatal(items)
	}
	if e := os.Remove(filepath.Join(path, "uv-receipt.toml")); e != nil {
		t.Fatal(e)
	}
	items = PythonTools(run)
	if items[0].LastChange.Source != TimeDirectoryEntry {
		t.Fatal(items)
	}
}

func TestDevDiscoverAssetsDeduplicatesAndMeasures(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("original desktop user home is not controlled by HOME")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("PATH", t.TempDir())
	t.Setenv("WORKON_HOME", filepath.Join(home, "no-venvs"))
	t.Setenv("XDG_CACHE_HOME", filepath.Join(home, "cache"))
	base, env := filepath.Join(home, "base"), filepath.Join(home, "env")
	devFixtureWrite(t, filepath.Join(base, "data"), "base")
	devFixtureWrite(t, filepath.Join(env, "data"), "env")
	t.Setenv("CONDA_PREFIX", base)
	modules := filepath.Join(home, "global", "node_modules")
	uv := filepath.Join(home, "uv")
	devFixtureWrite(t, filepath.Join(uv, "ruff", "uv-receipt.toml"), "receipt")
	run := devFixtureRun(map[string]ProcRun{
		"conda info --json":                {Stdout: []byte(devFixtureJSON(map[string]any{"envs": []string{base, env, env}, "root_prefix": base})), OK: true},
		"npm prefix --global":              {Stdout: []byte(filepath.Dir(modules)), OK: true},
		"npm ls --global --depth=0 --json": {Stdout: []byte(`{"dependencies":{}}`), OK: true},
		"pnpm root --global":               {Stdout: []byte(modules), OK: true},
		"uv tool dir":                      {Stdout: []byte(uv), OK: true},
		"pipx list --json":                 {Stdout: []byte(devFixtureJSON(map[string]any{"venvs": map[string]any{"duplicate": map[string]any{"metadata": map[string]any{"environment": env}}}})), OK: true},
	})
	out := DiscoverDevAssetsWith(run)
	if len(out.CondaEnvs) != 2 || len(out.PythonEnvs) != 0 || len(out.NodePackages) != 0 || len(out.PythonTools) != 1 {
		t.Fatalf("unexpected groups: %+v", out)
	}
	if out.Total.LogicalBytes != 14 || out.Total.FileCount != 3 || out.CondaEnvs[0].Size.LogicalBytes != 4 || out.CondaEnvs[1].Size.LogicalBytes != 3 || out.PythonTools[0].Size.LogicalBytes != 7 || out.RemovableExclusive != 10 {
		t.Fatalf("measurement or reclaimability mismatch: %+v", out)
	}
}
