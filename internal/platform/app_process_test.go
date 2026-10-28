package platform

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAppProcessLongCommandStaging(t *testing.T) {
	cache := t.TempDir()
	app, cwd := `C:\Program Files\runtime\python.exe`, `C:\source with spaces`
	args := []string{`a"b`, `tail\`, "", strings.Repeat("字", 1200), "x & $env:USERPROFILE"}
	line := appExecutableLine(app, args)
	prepared, err := prepareAppTokenCommand(app, line, cwd, cache, "powershell.exe", func(string) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	if prepared.application != "powershell.exe" || strings.Contains(prepared.line, args[3]) {
		t.Fatal("arguments became script source")
	}
	data, err := os.ReadFile(filepath.Join(prepared.staging, "launch.ps1.json"))
	if err != nil {
		t.Fatal(err)
	}
	var payload struct{ Application, Arguments, CWD string }
	if err := json.Unmarshal(data, &payload); err != nil {
		t.Fatal(err)
	}
	if payload.Application != app || payload.CWD != cwd || payload.Arguments != strings.TrimPrefix(line, QuoteWindowsArgument(app)+" ") {
		t.Fatalf("payload=%+v", payload)
	}
	data, err = os.ReadFile(filepath.Join(prepared.staging, "launch.ps1"))
	if err != nil || string(data) != appTokenDispatcher {
		t.Fatalf("dispatcher: %v", err)
	}
	staging := prepared.staging
	prepared.cleanup()
	if _, err := os.Stat(staging); !os.IsNotExist(err) {
		t.Fatalf("staging retained: %v", err)
	}
	denied := errors.New("linked ancestor")
	if _, err := prepareAppTokenCommand(app, line, cwd, cache, "powershell.exe", func(string) error { return denied }); !errors.Is(err, denied) {
		t.Fatalf("rejection lost: %v", err)
	}
	entries, err := os.ReadDir(filepath.Join(cache, "QuickCleaner", "command-staging"))
	if err != nil || len(entries) != 0 {
		t.Fatalf("failed launch left staging: %v %v", entries, err)
	}
}

func TestAppProcessCommandLimitsAndMatching(t *testing.T) {
	for _, tc := range []struct {
		line  string
		valid bool
	}{{strings.Repeat("x", 32766), true}, {strings.Repeat("x", 32767), false}, {strings.Repeat("😀", 16384), false}, {"x\x00y", false}} {
		if got := appNativeCommandValid("cmd.exe", tc.line); got != tc.valid {
			t.Fatalf("native limit len=%d got=%v", len(tc.line), got)
		}
	}
	for _, tc := range []struct {
		image, name, dir, stem string
		want                   bool
	}{
		{`C:\App\helper.exe`, "helper.exe", `c:/app`, "", true},
		{`C:\Application\helper.exe`, "helper.exe", `c:/app`, "", false},
		{`C:\Temp\unins000.tmp`, "unins000.tmp", "", "unins000", true},
		{"", "powershell.exe", "", "", false},
		{"", "ΟΣ", "", "ος", true},
		{"", "ΟΣ.tmp", "", "ος", false},
		{"", "ΟΣ.tmp", "", "οσ", true},
		{"", "other.exe", "", "unins000", false},
	} {
		if got := appRelatedProcess(tc.image, tc.name, tc.dir, tc.stem); got != tc.want {
			t.Fatalf("match %+v=%v", tc, got)
		}
	}
}
