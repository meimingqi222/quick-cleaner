//go:build windows

package platform

import "syscall"

// A query handle is enough to tell a live process from a recycled PID.
func macWalkPIDAlive(pid int) bool {
	if pid <= 0 {
		return false
	}
	const processQueryLimitedInformation = 0x1000
	handle, err := syscall.OpenProcess(processQueryLimitedInformation, false, uint32(pid))
	if err == nil {
		_ = syscall.CloseHandle(handle)
		return true
	}
	// Access denied still means a live process, matching Unix EPERM.
	return err == syscall.Errno(5)
}
