package core

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestProcWindowsShimTreeDeadline(t *testing.T) {
	for _, mode := range []string{"parent", "exit-parent"} {
		t.Run(mode, func(t *testing.T) {
			dir := t.TempDir()
			shim := filepath.Join(dir, "tree probe.cmd")
			script := fmt.Sprintf("@echo off\r\n\"%s\" -test.run=^TestProcHelper$ -- --qc-proc %s \"%s\"\r\n", os.Args[0], mode, dir)
			if err := os.WriteFile(shim, []byte(script), 0600); err != nil {
				t.Fatal(err)
			}
			start := time.Now()
			_, ok := RunToolWithTimeout(shim, nil, 2*time.Second)
			elapsed := time.Since(start)
			if _, err := os.Stat(filepath.Join(dir, "ready")); err != nil {
				t.Fatalf("shim descendant did not start: %v", err)
			}
			if ok || elapsed >= 3*time.Second {
				t.Fatalf("shim deadline: transport=%v elapsed=%s", ok, elapsed)
			}
			time.Sleep(4 * time.Second)
			if _, err := os.Stat(filepath.Join(dir, "finished")); !os.IsNotExist(err) {
				t.Fatal("shim descendant survived timeout")
			}
		})
	}
}
