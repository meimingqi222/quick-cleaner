package core

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/meimingqi222/quick-cleaner/internal/platform"
)

type caskFixture struct {
	prefix, home, selected, receipt, config, definition, brew string
	app                                                       InstalledApp
}

func caskWrite(t *testing.T, path string, bytes []byte) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, bytes, 0700); err != nil {
		t.Fatal(err)
	}
}

func newCaskFixture(t *testing.T) caskFixture {
	t.Helper()
	root := t.TempDir()
	f := caskFixture{prefix: filepath.Join(root, "brew"), home: filepath.Join(root, "home")}
	f.selected = filepath.Join(f.home, "Applications", "Selected.app")
	f.receipt = filepath.Join(f.prefix, "Caskroom", "renamed", ".metadata", "INSTALL_RECEIPT.json")
	f.config = filepath.Join(filepath.Dir(f.receipt), "config.json")
	f.definition = filepath.Join(filepath.Dir(f.receipt), "1.2", "20261016090000", "Casks", "renamed.json")
	f.brew = filepath.Join(f.prefix, "bin", "brew")
	caskWrite(t, f.receipt, []byte(`{"source":{"tap":"homebrew/cask","version":"1.2"},"uninstall_flight_blocks":false,"uninstall_artifacts":[{"app":["Original.app",{"target":"Selected.app"}]},{"zap":[{"trash":"~/Documents"}]}]}`))
	config, _ := json.Marshal(map[string]any{"explicit": map[string]any{"appdir": filepath.ToSlash(filepath.Dir(f.selected))}})
	caskWrite(t, f.config, config)
	caskWrite(t, f.definition, []byte(`{}`))
	caskWrite(t, f.brew, []byte("#!/bin/sh\nexit 0\n"))
	if err := os.MkdirAll(f.selected, 0700); err != nil {
		t.Fatal(err)
	}
	f.app = InstalledApp{InstallLocation: &f.selected, RegistrySubpath: "com.example.main"}
	return f
}

func (f caskFixture) metadata(string) (*string, *string) { return &f.app.RegistrySubpath, nil }

func TestAppCaskUninstallerReceiptBoundContract(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Homebrew cask fixture; macOS product path")
	}
	for _, scenario := range []string{"no-owner", "other-copy", "multi-app", "unsupported-tap", "old-token", "ruby", "corrupt", "invalid-utf8", "missing", "mismatch-id", "changed-receipt", "nonzero", "timeout", "app-remains", "receipt-remains", "definition-remains", "success"} {
		t.Run(scenario, func(t *testing.T) {
			f := newCaskFixture(t)
			switch scenario {
			case "no-owner":
				if err := os.RemoveAll(filepath.Join(f.prefix, "Caskroom")); err != nil {
					t.Fatal(err)
				}
			case "other-copy":
				p := filepath.Join(f.home, "Other", "Selected.app")
				f.app.InstallLocation = &p
			case "multi-app":
				caskWrite(t, f.definition, []byte(`{"artifacts":[{"app":["Selected.app"]},{"app":["Companion.app"]}]}`))
			case "unsupported-tap":
				b, _ := os.ReadFile(f.receipt)
				v := caskTestJSON(t, string(b))
				v["source"] = map[string]any{"tap": "custom/cask", "version": "1.2"}
				b, _ = json.Marshal(v)
				caskWrite(t, f.receipt, b)
			case "old-token":
				caskWrite(t, f.definition, []byte(`{"old_tokens":["old"]}`))
			case "ruby":
				if err := os.Rename(f.definition, f.definition[:len(f.definition)-4]+"rb"); err != nil {
					t.Fatal(err)
				}
			case "corrupt":
				caskWrite(t, f.receipt, []byte(`{`))
			case "invalid-utf8":
				caskWrite(t, f.definition, []byte{'{', '"', 'x', '"', ':', '"', 0xff, '"', '}'})
			case "missing":
				if err := os.Remove(f.config); err != nil {
					t.Fatal(err)
				}
			}
			calls := 0
			metadata := func(p string) (*string, *string) {
				if scenario == "mismatch-id" {
					s := "other.id"
					return &s, nil
				}
				if scenario == "changed-receipt" {
					b, _ := os.ReadFile(f.receipt)
					caskWrite(t, f.receipt, append(b, ' '))
				}
				return f.metadata(p)
			}
			// 传给 brew 的是解开了别名之后的可执行文件路径：macOS 的 /var 指向
			// /private/var，夹具根就在这层别名下。
			expectedBrew, err := filepath.EvalSymlinks(f.brew)
			if err != nil {
				t.Fatal(err)
			}
			run := func(_ context.Context, brew, home, token string) (platform.ProcessResult, bool) {
				calls++
				if brew != expectedBrew || home != f.home || token != "renamed" {
					t.Fatalf("wrong command %s %s %s", brew, home, token)
				}
				if scenario == "nonzero" {
					return platform.ProcessResult{Stderr: []byte("refused")}, true
				}
				if scenario == "timeout" {
					return platform.ProcessResult{}, false
				}
				for _, p := range []string{f.selected, f.receipt, f.definition} {
					if scenario == "app-remains" && p == f.selected || scenario == "receipt-remains" && p == f.receipt || scenario == "definition-remains" && p == f.definition {
						continue
					}
					if err := os.RemoveAll(p); err != nil {
						t.Fatal(err)
					}
				}
				return platform.ProcessResult{OK: true}, true
			}
			handled, err := runAppCaskUninstaller(context.Background(), f.app, f.home, []string{f.prefix}, metadata, run)
			if scenario == "no-owner" || scenario == "other-copy" {
				if handled || err != nil || calls != 0 {
					t.Fatalf("%v %v calls=%d", handled, err, calls)
				}
				return
			}
			if scenario == "success" {
				if !handled || err != nil || calls != 1 {
					t.Fatalf("%v %v calls=%d", handled, err, calls)
				}
				return
			}
			if err == nil {
				t.Fatal("refusal must prevent fallback")
			}
			attempt := scenario == "nonzero" || scenario == "timeout" || scenario == "app-remains" || scenario == "receipt-remains" || scenario == "definition-remains"
			if attempt && (!handled || calls != 1) || !attempt && calls != 0 {
				t.Fatalf("%v %v calls=%d", handled, err, calls)
			}
			if !attempt && caskAbsent(f.selected) {
				t.Fatal("refusal deleted selected app")
			}
		})
	}
}

func TestAppCaskUninstallerMultipleOwnersAndLatestDefinition(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Homebrew cask fixture; macOS product path")
	}
	f := newCaskFixture(t)
	newer := filepath.Join(filepath.Dir(filepath.Dir(filepath.Dir(f.definition))), "20261016100000", "Casks", "renamed.json")
	caskWrite(t, newer, []byte(`{"artifacts":[{"app":["Selected.app"]},{"app":["Companion.app"]}]}`))
	records, err := caskInventory(f.prefix, f.home, f.app.RegistrySubpath)
	if err != nil || len(records) != 1 || records[0].Definition != newer || records[0].Scope.Authorizes(filepath.ToSlash(f.selected)) {
		t.Fatalf("%+v %v", records, err)
	}
	caskWrite(t, newer, []byte(`{}`))
	run := func(context.Context, string, string, string) (platform.ProcessResult, bool) {
		t.Fatal("ambiguous owner executed")
		return platform.ProcessResult{}, false
	}
	if _, err := runAppCaskUninstaller(context.Background(), f.app, f.home, []string{f.prefix, f.prefix}, f.metadata, run); err == nil {
		t.Fatal("multiple owners accepted")
	}
}

// Caskroom 里早期安装的 cask 只有 .metadata/<version>/<timestamp>/Casks 清单，没有
// INSTALL_RECEIPT.json。归属盘点是为了找「谁拥有所选应用」，这份缺失不能把整次卸载
// 判死——否则卸载无关应用也会报
// "Homebrew evidence missing: .../INSTALL_RECEIPT.json"。
//
// 但「不当错误」不等于「当不存在」：旧记录仍要凭清单里的 app artifact 认领归属，
// 并且因为拿不到 source 证据，Supported 恒为 false，永不授权卸载。
func TestAppCaskUninstallerIgnoresReceiptlessLegacyCasks(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Homebrew cask fixture; macOS product path")
	}
	f := newCaskFixture(t)
	legacy := filepath.Join(f.prefix, "Caskroom", "legacy", ".metadata")
	caskWrite(t, filepath.Join(legacy, "config.json"), []byte(`{"default":{"appdir":"/Applications"}}`))
	caskWrite(t, filepath.Join(legacy, "0.12.6-2", "20230407104235.511", "Casks", "legacy.json"),
		[]byte(`{"artifacts":[{"uninstall":[{"pkgutil":"org.example.legacy"}]}]}`))
	caskWrite(t, filepath.Join(f.prefix, "Caskroom", "legacy", "0.12.6-2", "Legacy.pkg"), []byte("pkg"))

	records, err := caskInventory(f.prefix, f.home, f.app.RegistrySubpath)
	if err != nil {
		t.Fatalf("receipt-less legacy cask blocked the inventory: %v", err)
	}
	if len(records) != 2 {
		t.Fatalf("legacy record vanished from the owner list: %+v", records)
	}
	for _, record := range records {
		if record.Token == "legacy" && (record.Supported || len(record.Scope.Apps) != 0) {
			t.Fatalf("receipt-less record gained authority: %+v", record)
		}
	}
	// 收据缺失的那条与所选应用无关，卸载照常执行。
	calls := 0
	run := func(context.Context, string, string, string) (platform.ProcessResult, bool) {
		calls++
		for _, p := range []string{f.selected, f.receipt, f.definition} {
			if err := os.RemoveAll(p); err != nil {
				t.Fatal(err)
			}
		}
		return platform.ProcessResult{OK: true}, true
	}
	handled, err := runAppCaskUninstaller(context.Background(), f.app, f.home, []string{f.prefix}, f.metadata, run)
	if !handled || err != nil || calls != 1 {
		t.Fatalf("legacy record blocked an owned uninstall: handled=%v err=%v calls=%d", handled, err, calls)
	}
}
