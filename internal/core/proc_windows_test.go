package core

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/sys/windows"
)

func TestProcWindowsShimTreeDeadline(t *testing.T) {
	t.Parallel()
	for _, mode := range []string{"parent", "exit-parent"} {
		t.Run(mode, func(t *testing.T) {
			t.Parallel()
			dir := t.TempDir()
			shim := filepath.Join(dir, "tree probe.cmd")
			script := fmt.Sprintf("@echo off\r\n\"%s\" -test.run=^TestProcHelper$ -- --qc-proc %s \"%s\"\r\n", os.Args[0], mode, dir)
			if err := os.WriteFile(shim, []byte(script), 0600); err != nil {
				t.Fatal(err)
			}
			start := time.Now()
			_, ok := RunToolWithTimeout(shim, nil, 2*time.Second)
			elapsed := time.Since(start)
			pid := readLeafPID(t, dir)
			if ok || elapsed >= 3*time.Second {
				t.Fatalf("shim deadline: transport=%v elapsed=%s", ok, elapsed)
			}
			if !waitProcessGone(pid, 3*time.Second) {
				t.Fatalf("shim descendant %d survived timeout", pid)
			}
		})
	}
}

// waitProcessGone returns as soon as pid has exited, so a passing check costs
// only the kill latency instead of a fixed sleep.
func waitProcessGone(pid int, budget time.Duration) bool {
	h, err := windows.OpenProcess(windows.SYNCHRONIZE, false, uint32(pid))
	if err != nil {
		// ERROR_INVALID_PARAMETER: no such process any more.
		return errors.Is(err, windows.ERROR_INVALID_PARAMETER)
	}
	defer windows.CloseHandle(h)
	ev, err := windows.WaitForSingleObject(h, uint32(budget.Milliseconds()))
	return err == nil && ev == windows.WAIT_OBJECT_0
}
