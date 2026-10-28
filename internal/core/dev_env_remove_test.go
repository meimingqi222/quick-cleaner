package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

func devRemovalFixture(t *testing.T) DevAssetItem {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "environment")
	if err := os.Mkdir(path, 0755); err != nil {
		t.Fatal(err)
	}
	return DevAssetItem{Path: path, Kind: DevAssetKind{Type: DevCondaEnv, Name: "work"}, Source: AssetTool, InUse: SpotClear}
}

func devTestJSON(value any) ProcRun {
	data, _ := json.Marshal(value)
	return ProcRun{OK: true, Stdout: data}
}

func TestDevRemovalAuthorityGates(t *testing.T) {
	for _, tc := range []struct {
		name string
		edit func(*DevAssetItem)
		want RemovalRefusal
	}{
		{"base", func(i *DevAssetItem) { i.Kind.IsBase = true }, RemovalProtectedEnvironment},
		{"active", func(i *DevAssetItem) { i.IsActiveEnv = true }, RemovalProtectedEnvironment},
		{"layout", func(i *DevAssetItem) { i.Source = AssetLayout }, RemovalUnverifiedSource},
		{"busy", func(i *DevAssetItem) { i.InUse = SpotBusy }, RemovalBusy},
		{"unknown", func(i *DevAssetItem) { i.InUse = SpotUnknown }, RemovalBusy},
		{"bun", func(i *DevAssetItem) { i.Kind.Type = DevNodePackage; i.Kind.Manager = "bun" }, RemovalUnsupportedTool},
		{"interpreter", func(i *DevAssetItem) { i.Kind.Type = DevPythonInterpreter; i.Kind.Scope = SiteUser }, RemovalUnsupportedTool},
	} {
		t.Run(tc.name, func(t *testing.T) {
			i := devRemovalFixture(t)
			tc.edit(&i)
			got := removeDevAssetWith(i, func(string, []string, time.Duration) (ProcRun, bool) {
				t.Fatal("refused target reached tool runner")
				return ProcRun{}, false
			}, nil)
			if got.Status != AssetRefused || got.Refusal != tc.want {
				t.Fatalf("got %+v, want %s", got, tc.want)
			}
			if _, err := os.Stat(i.Path); err != nil {
				t.Fatal("refused target changed:", err)
			}
		})
	}
}

func TestDevRemovalMambaAndCompletion(t *testing.T) {
	for _, mode := range []string{"removed", "remains", "failed", "timeout", "base", "unregistered", "unreadable"} {
		t.Run(mode, func(t *testing.T) {
			i := devRemovalFixture(t)
			run := func(program string, args []string, timeout time.Duration) (ProcRun, bool) {
				if program == "conda" {
					if mode == "unreadable" {
						return ProcRun{OK: true, Stdout: []byte("broken")}, true
					}
					return ProcRun{}, false
				}
				if program != "mamba" {
					t.Fatalf("unexpected executor %s", program)
				}
				if strings.Join(args, " ") == "info --json" {
					if mode == "unreadable" {
						return ProcRun{}, false
					}
					inventory := map[string]any{"envs": []string{i.Path}}
					if mode == "base" {
						inventory["root_prefix"] = i.Path
					}
					if mode == "unregistered" {
						inventory["envs"] = []string{}
					}
					return devTestJSON(inventory), true
				}
				if strings.Join(args, "|") != strings.Join([]string{"remove", "--prefix", i.Path, "--all", "--yes"}, "|") || timeout != 300*time.Second {
					t.Fatal("wrong mutation authority", args, timeout)
				}
				if mode == "timeout" {
					return ProcRun{}, false
				}
				if mode == "failed" {
					return ProcRun{Stderr: []byte("permission denied")}, true
				}
				if mode == "removed" {
					if err := os.Remove(i.Path); err != nil {
						t.Fatal(err)
					}
				}
				return ProcRun{OK: true}, true
			}
			got := removeDevAssetWith(i, run, func(string) bool { t.Fatal("command fell back to raw deletion"); return false })
			switch mode {
			case "removed":
				if got.Status != AssetRemoved {
					t.Fatal(got)
				}
			case "base":
				if got.Refusal != RemovalProtectedEnvironment {
					t.Fatal(got)
				}
			case "unregistered":
				if got.Refusal != RemovalNotRegistered {
					t.Fatal(got)
				}
			case "unreadable":
				if got.Refusal != RemovalToolUnreadable {
					t.Fatal(got)
				}
			default:
				want := map[string]string{"remains": "reported-success-but-remains", "failed": "command-failed", "timeout": "command-unavailable-or-timeout"}[mode]
				if got.Status != AssetFailed || got.Code != want {
					t.Fatalf("got %+v, want %s", got, want)
				}
			}
			if mode != "removed" {
				if _, err := os.Stat(i.Path); err != nil {
					t.Fatal("failed/refused target was deleted", err)
				}
			}
		})
	}
}

func TestDevRemovalRefusesSwitchedToolPaths(t *testing.T) {
	for _, channel := range []string{"npm", "pnpm", "uv", "pipx"} {
		t.Run(channel, func(t *testing.T) {
			i := devRemovalFixture(t)
			i.Kind = DevAssetKind{Type: DevNodePackage, Name: "tool", Manager: channel}
			if channel == "uv" || channel == "pipx" {
				i.Kind.Type = DevPythonTool
				i.Kind.ToolKind = PythonToolKind(channel)
			}
			other := filepath.Join(filepath.Dir(i.Path), "other-prefix")
			run := func(_ string, args []string, _ time.Duration) (ProcRun, bool) {
				switch strings.Join(args, " ") {
				case "prefix --global", "root --global", "tool dir":
					return ProcRun{OK: true, Stdout: []byte(other)}, true
				case "ls --global --depth=0 --json":
					return devTestJSON(map[string]any{"dependencies": map[string]any{"tool": map[string]string{"version": "1"}}}), true
				case "list --json":
					return devTestJSON(map[string]any{"venvs": map[string]any{"tool": map[string]any{"metadata": map[string]string{"environment": other}}}}), true
				}
				t.Fatal("changed prefix reached mutation", args)
				return ProcRun{}, false
			}
			if got := removeDevAssetWith(i, run, nil); got.Status != AssetRefused || got.Refusal != RemovalNotRegistered {
				t.Fatal(got)
			}
			if _, err := os.Stat(i.Path); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestDevPipRemovalUsesOwningInterpreterAndFreshMetadata(t *testing.T) {
	for _, mode := range []string{"removed", "remains", "failed", "no-pip", "invalid-name", "missing"} {
		t.Run(mode, func(t *testing.T) {
			i := devRemovalFixture(t)
			i.Kind = DevAssetKind{Type: DevPipPackage, Name: "stale-cached-name", Python: filepath.Join(filepath.Dir(i.Path), "own-python")}
			name := "fresh-distribution"
			if mode == "invalid-name" {
				name = "invalid;distribution"
			}
			if err := os.WriteFile(filepath.Join(i.Path, "METADATA"), []byte("Name: "+name+"\nVersion: 1\n\nName: body-not-a-header"), 0600); err != nil {
				t.Fatal(err)
			}
			if mode == "missing" {
				if err := os.RemoveAll(i.Path); err != nil {
					t.Fatal(err)
				}
			}
			run := func(program string, args []string, timeout time.Duration) (ProcRun, bool) {
				if program != i.Kind.Python {
					t.Fatal("wrong interpreter", program)
				}
				if strings.Join(args, " ") == "-m pip --version" {
					return ProcRun{OK: mode != "no-pip"}, true
				}
				if strings.Join(args, " ") != "-m pip uninstall -y fresh-distribution" || timeout != 300*time.Second {
					t.Fatal("wrong pip mutation", args, timeout)
				}
				if mode == "removed" {
					if err := os.RemoveAll(i.Path); err != nil {
						t.Fatal(err)
					}
				}
				return ProcRun{OK: mode != "failed"}, true
			}
			got := removeDevAssetWith(i, run, func(string) bool { t.Fatal("pip fell back to raw delete"); return false })
			want := map[string]RemovalOutcome{
				"removed": {Status: AssetRemoved}, "remains": {Status: AssetFailed, Code: "reported-success-but-remains"}, "failed": {Status: AssetFailed, Code: "command-failed"},
				"no-pip": {Status: AssetRefused, Refusal: RemovalToolUnavailable}, "invalid-name": {Status: AssetRefused, Refusal: RemovalToolUnreadable}, "missing": {Status: AssetRefused, Refusal: RemovalNotRegistered},
			}[mode]
			if got.Status != want.Status || got.Refusal != want.Refusal || got.Code != want.Code {
				t.Fatalf("got %+v, want %+v", got, want)
			}
			if mode != "removed" && mode != "missing" {
				if _, err := os.Stat(i.Path); err != nil {
					t.Fatal("failed/refused pip target deleted", err)
				}
			}
		})
	}
}

func TestRemoveDevAssetVenvIdentityAndProtectedChildren(t *testing.T) {
	for _, mode := range []string{"removed", "protected-child", "replaced", "no-cfg", "unknown-identity", "linked-root", "linked-ancestor"} {
		t.Run(mode, func(t *testing.T) {
			i := devRemovalFixture(t)
			i.Source = AssetLayout
			i.Kind = DevAssetKind{Type: DevVirtualEnv, Name: "venv", Identity: CaptureVenvIdentity(i.Path)}
			if i.Kind.Identity == nil {
				t.Fatal("fixture lacks stable directory identity")
			}
			if err := os.WriteFile(filepath.Join(i.Path, "pyvenv.cfg"), []byte("home = python\n"), 0600); err != nil {
				t.Fatal(err)
			}
			if !i.Kind.Identity.Recheck(i.Path) {
				t.Fatal("in-place cfg change invalidated object identity")
			}
			switch mode {
			case "protected-child":
				keep := filepath.Join(i.Path, "keep")
				if err := os.WriteFile(keep, []byte("sentinel"), 0600); err != nil {
					t.Fatal(err)
				}
				old := CurrentWhitelist()
				t.Cleanup(func() { ReloadWhitelist(old) })
				ReloadWhitelist([]string{keep})
			case "replaced":
				if err := os.Rename(i.Path, i.Path+"-original"); err != nil {
					t.Fatal(err)
				}
				if err := os.Mkdir(i.Path, 0755); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filepath.Join(i.Path, "pyvenv.cfg"), []byte("home = new-python"), 0600); err != nil {
					t.Fatal(err)
				}
			case "no-cfg":
				if err := os.Remove(filepath.Join(i.Path, "pyvenv.cfg")); err != nil {
					t.Fatal(err)
				}
			case "unknown-identity":
				i.Kind.Identity = nil
			case "linked-root", "linked-ancestor":
				link := filepath.Join(filepath.Dir(i.Path), "link")
				target := i.Path
				if mode == "linked-ancestor" {
					target = filepath.Dir(i.Path)
				}
				if err := os.Symlink(target, link); err != nil {
					t.Skip("directory links unavailable:", err)
				}
				if mode == "linked-root" {
					i.Path = link
				} else {
					i.Path = filepath.Join(link, "environment")
				}
				if CaptureVenvIdentity(i.Path) != nil {
					t.Fatal("redirected path obtained deletion identity")
				}
			}
			got := RemoveDevAsset(i)
			if mode == "removed" {
				if got.Status != AssetRemoved {
					t.Fatal(got)
				}
				if _, err := os.Lstat(i.Path); !os.IsNotExist(err) {
					t.Fatal("venv remains", err)
				}
			} else if mode == "protected-child" {
				if got.Status != AssetFailed {
					t.Fatal(got)
				}
				if data, err := os.ReadFile(filepath.Join(i.Path, "keep")); err != nil || string(data) != "sentinel" {
					t.Fatal("protected child changed", err)
				}
			} else {
				if got.Status != AssetRefused || got.Refusal != RemovalNotRegistered {
					t.Fatal(got)
				}
				if _, err := os.Stat(i.Path); err != nil {
					t.Fatal("refused venv removed", err)
				}
			}
		})
	}
}

func TestRemoveDevAssetRunsNativeTool(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("POSIX CLI fixture; Windows runner uses the same bounded ToolCommand API")
	}
	root := t.TempDir()
	prefix := filepath.Join(root, "prefix")
	path := filepath.Join(prefix, "node_modules", "@qc", "tool")
	if err := os.MkdirAll(path, 0755); err != nil {
		t.Fatal(err)
	}
	script := `#!/bin/sh
case "$1" in
 prefix) printf '%s\n' "$QC_DEV_TEST_PREFIX" ;;
 ls) printf '%s\n' '{"dependencies":{"@qc/tool":{"version":"1.2.3"}}}'; exit 1 ;;
 uninstall) [ "$2" = --global ] && [ "$3" = @qc/tool ] && rmdir "$QC_DEV_TEST_PREFIX/node_modules/@qc/tool" ;;
 *) exit 2 ;;
esac
`
	if err := os.WriteFile(filepath.Join(root, "npm"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", root+string(os.PathListSeparator)+os.Getenv("PATH"))
	t.Setenv("QC_DEV_TEST_PREFIX", prefix)
	i := DevAssetItem{Path: path, Source: AssetTool, Kind: DevAssetKind{Type: DevNodePackage, Manager: "npm", Name: "@qc/tool"}, InUse: SpotClear}
	if got := RemoveDevAsset(i); got.Status != AssetRemoved {
		t.Fatal(got)
	}
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		t.Fatal("tool did not remove exact package", err)
	}
	if _, err := os.Stat(filepath.Dir(path)); err != nil {
		t.Fatal("tool removed package parent", err)
	}
}

func TestDevEvidenceAgeUsesWholeDaysNotLastUse(t *testing.T) {
	for _, tc := range []struct {
		at, now time.Time
		days    uint64
	}{
		{time.Unix(0, 900), time.Unix(86_400, 899), 0},
		{time.Unix(0, 900), time.Unix(86_400, 900), 1},
		{time.Unix(86_401, 0), time.Unix(86_400, 0), 0},
		{time.Date(1601, 1, 1, 0, 0, 0, 0, time.UTC), time.Date(2001, 1, 1, 0, 0, 0, 0, time.UTC), 146_097},
	} {
		if got := (TimedEvidence{At: tc.at, Source: TimeDirectoryEntry}).AgeDays(tc.now); got != tc.days {
			t.Fatalf("%s to %s: got %d days, want %d", tc.at, tc.now, got, tc.days)
		}
	}
}
