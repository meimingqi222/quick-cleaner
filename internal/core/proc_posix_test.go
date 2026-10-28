//go:build !windows

package core

import (
	"testing"
	"time"
)

func TestProcSignal(t *testing.T) {
	r, ok := RunWithTimeout("/bin/sh", []string{"-c", "kill -TERM $$"}, time.Second)
	if !ok || r.OK || r.ExitCode != nil {
		t.Fatalf("signal result: %+v transport=%v", r, ok)
	}
}
