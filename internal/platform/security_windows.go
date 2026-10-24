package platform

import (
	"os"
	"runtime"
	"strings"
	"syscall"
	"unsafe"
)

var shellExecute = syscall.NewLazyDLL("shell32.dll").NewProc("ShellExecuteW")

func IsElevated() bool {
	token, err := fileopsProcessToken()
	if err != nil {
		return false
	}
	defer token.Close()
	var elevated, size uint32
	return syscall.GetTokenInformation(token, syscall.TokenElevation, (*byte)(unsafe.Pointer(&elevated)), 4, &size) == nil && elevated != 0
}

// RelaunchAsAdminIfNeeded exits only after Windows accepts the UAC relaunch.
// Cancelling UAC leaves the current process running without elevation.
func RelaunchAsAdminIfNeeded() bool {
	if IsElevated() {
		return true
	}
	exe, err := os.Executable()
	if err != nil {
		return false
	}
	captured := originalUser{}
	if home, err := os.UserHomeDir(); err == nil {
		captured.home, captured.hasHome = home, true
	} else if home, ok := os.LookupEnv("USERPROFILE"); ok {
		captured.home, captured.hasHome = home, true
	}
	captured.sid, captured.hasSID = fileopsProcessSID()
	args := elevationArgs(os.Args[1:], captured)
	quoted := make([]string, len(args))
	for i, arg := range args {
		quoted[i] = QuoteWindowsArgument(arg)
	}
	program, err := syscall.UTF16PtrFromString(exe)
	if err != nil {
		return false
	}
	parameters, err := syscall.UTF16PtrFromString(strings.Join(quoted, " "))
	if err != nil {
		return false
	}
	verb, _ := syscall.UTF16PtrFromString("runas")
	result, _, _ := shellExecute.Call(0, uintptr(unsafe.Pointer(verb)), uintptr(unsafe.Pointer(program)), uintptr(unsafe.Pointer(parameters)), 0, 1)
	runtime.KeepAlive(verb)
	runtime.KeepAlive(program)
	runtime.KeepAlive(parameters)
	if result > 32 {
		os.Exit(0)
	}
	return false
}
