//go:build !windows

package core

import (
	"errors"
	"syscall"
	"testing"
	"time"
)

// waitProcessGone polls kill(pid, 0) until the process is reaped. The killed
// leaf is reparented to init/launchd, which reaps it promptly.
func waitProcessGone(pid int, budget time.Duration) bool {
	deadline := time.Now().Add(budget)
	for {
		if err := syscall.Kill(pid, 0); errors.Is(err, syscall.ESRCH) {
			return true
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestProcSignal(t *testing.T) {
	r, ok := RunWithTimeout("/bin/sh", []string{"-c", "kill -TERM $$"}, time.Second)
	if !ok || r.OK || r.ExitCode != nil {
		t.Fatalf("signal result: %+v transport=%v", r, ok)
	}
}
