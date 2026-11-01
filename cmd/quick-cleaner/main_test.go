package main

import (
	"runtime"
	"testing"
)

func TestHelperDispatchNeverInitializesGUIForInvalidOrUnsupportedMode(t *testing.T) {
	for _, args := range [][]string{{"--fanhelper", "extra"}, {"--fanhelper-install", "--no-elevate"}} {
		if got := run(args); got != 2 {
			t.Fatalf("helper arguments %v: exit %d", args, got)
		}
	}
	if runtime.GOOS != "darwin" {
		for _, flag := range []string{"--fanhelper", "--fanhelper-install"} {
			if got := run([]string{flag}); got != 1 {
				t.Fatalf("unsupported helper %s: exit %d", flag, got)
			}
		}
	}
}
func TestVersionDispatchDoesNotInitializeGUI(t *testing.T) {
	if run([]string{"--version"}) != 0 {
		t.Fatal("version dispatch failed")
	}
}

func TestCLIDispatchDoesNotInitializeGUI(t *testing.T) {
	for _, args := range [][]string{{"cli", "--help"}, {"--help"}, {"cli", "junk", "categories", "--json"}} {
		if code := run(args); code != 0 {
			t.Fatalf("headless dispatch %v: exit %d", args, code)
		}
	}
	if code := run([]string{"cli", "unknown-command"}); code != 2 {
		t.Fatalf("invalid CLI must return usage error, got %d", code)
	}
}
