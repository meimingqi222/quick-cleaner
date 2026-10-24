//go:build windows

package main

import (
	"encoding/binary"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"unsafe"
)

func init() {
	if windowsTestBinary() {
		return
	}
	// --version and the helper flags print and exit. They need the parent
	// console. A GUI-subsystem build has none until AttachConsole.
	if windowsConsoleArgs() {
		attachParentConsole()
		return
	}
	// QC_WINDOWS_GUI marks the detached child. Relaunching again would loop.
	// A binary already linked with -H windowsgui never allocates a console.
	if os.Getenv("QC_WINDOWS_GUI") == "1" || windowsSubsystem() == 2 {
		hideOwnedConsole()
		return
	}
	// go run and ShellExecute keep a console for the whole process. Rust's
	// windows_subsystem attribute never creates one. Detach a child, then
	// leave: the parent console (go run, or the elevated flash) exits with us.
	hideOwnedConsole()
	if relaunchWithoutConsole() {
		os.Exit(0)
	}
}

func windowsTestBinary() bool {
	if strings.Contains(strings.ToLower(filepath.Base(os.Args[0])), ".test") {
		return true
	}
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "-test.") {
			return true
		}
	}
	return false
}

func windowsConsoleArgs() bool {
	if len(os.Args) != 2 {
		return false
	}
	switch os.Args[1] {
	case "--version", "--fanhelper", "--fanhelper-install":
		return true
	default:
		return false
	}
}

func relaunchWithoutConsole() bool {
	exe, err := os.Executable()
	if err != nil {
		return false
	}
	cmd := exec.Command(exe, os.Args[1:]...)
	cmd.Env = append(os.Environ(), "QC_WINDOWS_GUI=1")
	if dir, err := os.Getwd(); err == nil {
		cmd.Dir = dir
	}
	// DETACHED_PROCESS | CREATE_NO_WINDOW. HideWindow is not set: STARTUPINFO
	// SW_HIDE would also hide the real window on a GUI-subsystem build.
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: 0x08000008}
	return cmd.Start() == nil
}

func windowsSubsystem() int {
	exe, err := os.Executable()
	if err != nil {
		return 0
	}
	f, err := os.Open(exe)
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

func hideOwnedConsole() {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	user32 := syscall.NewLazyDLL("user32.dll")
	hwnd, _, _ := kernel32.NewProc("GetConsoleWindow").Call()
	if hwnd == 0 || !consoleOwned() {
		return
	}
	user32.NewProc("ShowWindow").Call(hwnd, 0) // SW_HIDE
	kernel32.NewProc("FreeConsole").Call()
}

func consoleOwned() bool {
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	var ids [64]uint32
	n, _, _ := kernel32.NewProc("GetConsoleProcessList").Call(uintptr(unsafe.Pointer(&ids[0])), uintptr(len(ids)))
	return n == 1
}

func attachParentConsole() {
	// A pipe from the parent is already a working stdout. Attaching the
	// console and swapping the handle would throw that pipe away.
	if stdHandleWorks(syscall.STD_OUTPUT_HANDLE) {
		return
	}
	kernel32 := syscall.NewLazyDLL("kernel32.dll")
	// ATTACH_PARENT_PROCESS. A process that already has a console fails this
	// and keeps the stdout Go opened.
	r, _, _ := kernel32.NewProc("AttachConsole").Call(^uintptr(0))
	if r == 0 {
		return
	}
	reopenConsole(syscall.STD_OUTPUT_HANDLE, &os.Stdout, "stdout")
	reopenConsole(syscall.STD_ERROR_HANDLE, &os.Stderr, "stderr")
}

func stdHandleWorks(which int) bool {
	h, err := syscall.GetStdHandle(which)
	if err != nil || h == 0 || h == syscall.InvalidHandle {
		return false
	}
	ft, err := syscall.GetFileType(syscall.Handle(h))
	return err == nil && ft != syscall.FILE_TYPE_UNKNOWN
}

func reopenConsole(which int, file **os.File, name string) {
	h, err := syscall.GetStdHandle(which)
	if err != nil || h == 0 || h == syscall.InvalidHandle {
		return
	}
	*file = os.NewFile(uintptr(h), name)
}
