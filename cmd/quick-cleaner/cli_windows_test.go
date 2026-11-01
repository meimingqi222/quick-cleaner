package main

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"syscall"
	"testing"
	"time"
)

func TestCLIConsoleAndGUISubsystemKeepPipesAndExitCodes(t *testing.T) {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "sentinel.txt"), []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, subsystem := range []string{"console", "gui"} {
		t.Run(subsystem, func(t *testing.T) {
			ctx, stop := context.WithTimeout(context.Background(), 2*time.Minute)
			defer stop()
			exe := filepath.Join(t.TempDir(), "quick-cleaner.exe")
			args := []string{"build", "-o", exe}
			if subsystem == "gui" {
				args = append(args, "-ldflags=-H=windowsgui")
			}
			args = append(args, ".")
			if out, err := exec.CommandContext(ctx, "go", args...).CombinedOutput(); err != nil {
				t.Fatalf("build: %v\n%s", err, out)
			}
			cmd := exec.CommandContext(ctx, exe, "cli", "search", "--root", root, "--query", "*.txt", "--json")
			cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
			out, err := cmd.Output()
			if err != nil {
				t.Fatalf("CLI pipe: %v\n%s", err, out)
			}
			var report struct {
				Rows []struct {
					Path string `json:"path"`
				} `json:"rows"`
			}
			if err := json.Unmarshal(out, &report); err != nil || len(report.Rows) != 1 || report.Rows[0].Path != filepath.Join(root, "sentinel.txt") {
				t.Fatalf("CLI lost/corrupted stdout pipe: %s (%v)", out, err)
			}
			cmd = exec.CommandContext(ctx, exe, "cli", "unknown-command")
			cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
			if err := cmd.Run(); err == nil || cmd.ProcessState.ExitCode() != 2 {
				t.Fatalf("CLI usage exit lost: %v; state=%v", err, cmd.ProcessState)
			}
		})
	}
}
