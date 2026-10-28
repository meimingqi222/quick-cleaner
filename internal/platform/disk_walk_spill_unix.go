//go:build darwin || linux

package platform

import (
	"errors"
	"syscall"
)

// kill(pid, 0) answers reachability. EPERM means another user's live process.
func macWalkPIDAlive(pid int) bool {
	err := syscall.Kill(pid, 0)
	return err == nil || errors.Is(err, syscall.EPERM)
}
