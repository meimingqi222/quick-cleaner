package platform

import (
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestUpdateBundleAndDevBuild(t *testing.T) {
	for _, tc := range []struct{ path, want string }{
		{"/Applications/My App.app/Contents/MacOS/bin", "/Applications/My App.app"},
		{"/A.app/Contents/Helpers/B.app/Contents/MacOS/b", "/A.app/Contents/Helpers/B.app"},
		{"/opt/app.application/bin", ""}, {"/usr/local/bin/app", ""},
	} {
		want := filepath.FromSlash(tc.want)
		if tc.want == "" {
			want = ""
		}
		got, ok := UpdateBundleForExecutable(filepath.FromSlash(tc.path))
		if got != want || ok != (tc.want != "") {
			t.Fatalf("%s: %s %v", tc.path, got, ok)
		}
	}
	for _, tc := range []struct {
		path string
		want bool
	}{
		{`D:\code\app\target\debug\app.exe`, true}, {"/tmp/app/TARGET/release/app", true},
		{`C:\Users\me\Programs\app.exe`, false}, {"/Applications/App.app/Contents/MacOS/app", false},
	} {
		if LooksLikeDevBuild(tc.path) != tc.want {
			t.Fatal(tc)
		}
	}
}

func TestUpdateBackupCleanupKeepsCurrentAndWrongKinds(t *testing.T) {
	root := t.TempDir()
	current := filepath.Join(root, "app.exe")
	if err := os.WriteFile(current, []byte("current"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(current+".old", []byte("old"), 0600); err != nil {
		t.Fatal(err)
	}
	RemoveUpdateBackup(current, false)
	if _, err := os.Stat(current + ".old"); !os.IsNotExist(err) {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(current); err != nil || string(data) != "current" {
		t.Fatal(string(data), err)
	}
	if err := os.Mkdir(current+".old", 0700); err != nil {
		t.Fatal(err)
	}
	RemoveUpdateBackup(current, false)
	if info, err := os.Stat(current + ".old"); err != nil || !info.IsDir() {
		t.Fatal(err)
	}
	RemoveUpdateBackup(current, true)
	if _, err := os.Stat(current + ".old"); !os.IsNotExist(err) {
		t.Fatal(err)
	}
	if err := os.Symlink(current, current+".old"); err == nil {
		RemoveUpdateBackup(current, false)
		if _, err := os.Lstat(current + ".old"); err != nil {
			t.Fatal("linked backup removed", err)
		}
	}
}

func TestMacUpdateHelperReplacementAndRollback(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("requires bash and POSIX fixture commands")
	}
	for _, rollback := range []bool{false, true} {
		name := "replacement"
		if rollback {
			name = "rollback"
		}
		t.Run(name, func(t *testing.T) {
			root := filepath.Join(t.TempDir(), "it's here $literal")
			current := filepath.Join(root, "Apps", "My App.app")
			payload := filepath.Join(root, "stage", "My App.app")
			bin := filepath.Join(root, "bin")
			for _, dir := range []string{current, payload, bin} {
				if err := os.MkdirAll(dir, 0700); err != nil {
					t.Fatal(err)
				}
			}
			if err := os.WriteFile(filepath.Join(current, "version"), []byte("old"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(payload, "version"), []byte("new"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(bin, "open"), []byte("#!/bin/sh\nprintf '%s' \"$1\" >> \"$OPEN_LOG\"\n"), 0700); err != nil {
				t.Fatal(err)
			}
			parent := exec.Command("true")
			if err := parent.Run(); err != nil {
				t.Fatal(err)
			}
			source := payload
			if rollback {
				source = filepath.Join(root, "missing.app")
			}
			log := filepath.Join(root, "apply-update.log")
			script := filepath.Join(root, "apply-update.sh")
			if err := os.WriteFile(script, []byte(MacosBundleReplaceScript(current, source, log, parent.Process.Pid)), 0700); err != nil {
				t.Fatal(err)
			}
			cmd := exec.Command("/bin/bash", script)
			openLog := filepath.Join(root, "open.log")
			cmd.Env = append(os.Environ(), "PATH="+bin+string(os.PathListSeparator)+os.Getenv("PATH"), "OPEN_LOG="+openLog)
			output, err := cmd.CombinedOutput()
			if (err != nil) != rollback {
				t.Fatalf("error=%v rollback=%v output=%s", err, rollback, output)
			}
			want := "new"
			if rollback {
				want = "old"
			}
			if data, err := os.ReadFile(filepath.Join(current, "version")); err != nil || string(data) != want {
				t.Fatal(string(data), err)
			}
			if data, err := os.ReadFile(openLog); err != nil || string(data) != current {
				t.Fatal(string(data), err)
			}
			if data, err := os.ReadFile(filepath.Join(payload, "version")); err != nil || string(data) != "new" {
				t.Fatal("payload changed", string(data), err)
			}
			if !rollback {
				if data, err := os.ReadFile(filepath.Join(current+".old", "version")); err != nil || string(data) != "old" {
					t.Fatal(string(data), err)
				}
			} else if _, err := os.Stat(current + ".old"); !os.IsNotExist(err) {
				t.Fatal("rollback did not restore backup", err)
			}
		})
	}
}

func TestWindowsUpdateScriptQuotesAndRetainsRecovery(t *testing.T) {
	script := WindowsReplaceScript(`C:\Program Files\O'Brien\app.exe`, `C:\cache\stage\app.exe`, `C:\cache\apply-update.log`, 4242)
	if !strings.Contains(script, `$exe = 'C:\Program Files\O''Brien\app.exe'`) {
		t.Fatal("unquoted executable")
	}
	last := -1
	for _, token := range []string{"Wait-Process -Id 4242", "Move-Item -LiteralPath $exe -Destination $old", "Copy-Item -LiteralPath $new -Destination $exe", "Move-Item -LiteralPath $old -Destination $exe"} {
		pos := strings.Index(script, token)
		if pos <= last {
			t.Fatal("missing or out of order:", token)
		}
		last = pos
	}
	if strings.Contains(script, "Remove-Item -LiteralPath $new") {
		t.Fatal("payload must remain available")
	}
}
