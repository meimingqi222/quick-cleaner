package platform

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestCommandWindowsResolution(t *testing.T) {
	a, b := t.TempDir(), t.TempDir()
	for _, p := range []string{filepath.Join(a, "tool.cmd"), filepath.Join(a, "tool.exe"), filepath.Join(b, "tool.exe")} {
		if err := os.WriteFile(p, []byte("x"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, tc := range []struct{ ext, want string }{{".CMD;.EXE", "tool.cmd"}, {".EXE;.CMD", "tool.exe"}} {
		p, ok := resolveToolIn("tool", a+";"+b, tc.ext)
		if !ok || !strings.EqualFold(p, filepath.Join(a, tc.want)) {
			t.Fatalf("resolved %q, %v", p, ok)
		}
	}
	if _, ok := resolveToolIn("tool.cmd", a, ""); !ok {
		t.Fatal("explicit extension")
	}
	if _, ok := ResolveToolProgram(a); ok {
		t.Fatal("directory resolved as file")
	}
	if err := os.Remove(filepath.Join(a, "tool.exe")); err != nil {
		t.Fatal(err)
	}
	p, ok := resolveToolIn("tool", a+";"+b, ".EXE;.CMD")
	if !ok || !strings.EqualFold(p, filepath.Join(a, "tool.cmd")) {
		t.Fatalf("PATH must be the outer loop: %q, %v", p, ok)
	}
}

func TestCommandWindowsShim(t *testing.T) {
	shim := filepath.Join(t.TempDir(), "probe shim.cmd")
	if err := os.WriteFile(shim, []byte("@echo off\r\necho [%~1]\r\necho [%~2]\r\necho [%~3]\r\necho stderr-probe 1>&2\r\nexit /b 7\r\n"), 0600); err != nil {
		t.Fatal(err)
	}
	cmd, ok := ToolCommand(shim, []string{"two words", `C:\some dir\`, ""})
	if !ok {
		t.Fatal("shim rejected")
	}
	r, ok := RunCommandTree(cmd, 5*time.Second)
	if !ok || r.OK || r.ExitCode == nil || *r.ExitCode != 7 || strings.ReplaceAll(string(r.Stdout), "\r\n", "\n") != "[two words]\n[C:\\some dir\\]\n[]\n" || !strings.Contains(string(r.Stderr), "stderr-probe") {
		t.Fatalf("shim result: %+v transport=%v stdout=%q", r, ok, r.Stdout)
	}
	for _, c := range "\"&|<>^%!\r\n\x00" {
		if _, ok := ToolCommand(shim, []string{"a" + string(c) + "b"}); ok {
			t.Fatalf("accepted %q", c)
		}
	}
}

func TestCommandWindowsNoConsole(t *testing.T) {
	cmd, ok := ToolCommand("powershell.exe", []string{"-NoProfile", "-NonInteractive", "-Command", `Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class QCConsole { [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow(); }'; [Console]::Write([QCConsole]::GetConsoleWindow().ToInt64())`})
	if !ok {
		t.Fatal("powershell.exe not resolved")
	}
	r, ok := RunCommandTree(cmd, 30*time.Second)
	if !ok || !r.OK || string(r.Stdout) != "0" {
		t.Fatalf("console probe failed: %+v transport=%v", r, ok)
	}
}
