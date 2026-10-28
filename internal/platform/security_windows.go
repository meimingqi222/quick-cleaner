package platform

import (
	"encoding/binary"
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
	show := uintptr(elevationShowCmd(imageSubsystem(exe)))
	result, _, _ := shellExecute.Call(0, uintptr(unsafe.Pointer(verb)), uintptr(unsafe.Pointer(program)), uintptr(unsafe.Pointer(parameters)), 0, show)
	runtime.KeepAlive(verb)
	runtime.KeepAlive(program)
	runtime.KeepAlive(parameters)
	if result > 32 {
		os.Exit(0)
	}
	return false
}

// imageSubsystem reads the PE subsystem. 3 is a console program, 2 is GUI.
func imageSubsystem(path string) int {
	f, err := os.Open(path)
	if err != nil {
		return 0
	}
	defer f.Close()
	var hdr [64]byte
	if _, err = f.Read(hdr[:]); err != nil || hdr[0] != 'M' || hdr[1] != 'Z' {
		return 0
	}
	pe := int64(binary.LittleEndian.Uint32(hdr[0x3C:]))
	var opt [96]byte
	if _, err = f.ReadAt(opt[:], pe); err != nil || binary.LittleEndian.Uint32(opt[:4]) != 0x00004550 {
		return 0
	}
	magic := binary.LittleEndian.Uint16(opt[24:])
	switch magic {
	case 0x10B:
		return int(binary.LittleEndian.Uint16(opt[24+64:]))
	case 0x20B:
		return int(binary.LittleEndian.Uint16(opt[24+68:]))
	default:
		return 0
	}
}
