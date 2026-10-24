package platform

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"
	"unsafe"
)

var fileopsShell = syscall.NewLazyDLL("shell32.dll")
var shellFileOperation = fileopsShell.NewProc("SHFileOperationW")
var shellEmptyRecycle = fileopsShell.NewProc("SHEmptyRecycleBinW")

type shellFileOp struct {
	Window   uintptr
	Function uint32
	From, To *uint16
	Flags    uint16
	Aborted  int32
	Mappings uintptr
	Title    *uint16
}

func MoveToTrash(path string) error {
	p, err := trashPath(path)
	if err != nil {
		return err
	}
	from, err := syscall.UTF16FromString(p)
	if err != nil {
		return err
	}
	from = append(from, 0)
	op := shellFileOp{Function: 3, From: &from[0], Flags: 0x0040 | 0x0010 | 0x0400 | 0x0004}
	rc, _, _ := shellFileOperation.Call(uintptr(unsafe.Pointer(&op)))
	runtime.KeepAlive(from)
	if rc != 0 {
		return fmt.Errorf("SHFileOperationW: 0x%08X", rc)
	}
	if op.Aborted != 0 {
		return errors.New("recycle operation aborted")
	}
	return nil
}

func fileopsProcessToken() (syscall.Token, error) {
	p, err := syscall.GetCurrentProcess()
	if err != nil {
		return 0, err
	}
	var token syscall.Token
	err = syscall.OpenProcessToken(p, syscall.TOKEN_QUERY, &token)
	return token, err
}

func fileopsProcessSID() (string, bool) {
	t, err := fileopsProcessToken()
	if err != nil {
		return "", false
	}
	defer t.Close()
	u, err := t.GetTokenUser()
	if err != nil {
		return "", false
	}
	s, err := u.User.Sid.String()
	return s, err == nil
}

func ForceDeleteAccess(path string) bool {
	if !IsElevated() {
		return false
	}
	run := func(tool string, args ...string) bool {
		cmd, ok := ToolCommand(tool, args)
		if !ok {
			return false
		}
		result, ok := RunCommandTree(cmd, 2*time.Second)
		return ok && result.OK
	}
	_ = run("takeown", "/f", path, "/a")
	args := []string{path}
	if sid, ok := fileopsProcessSID(); ok {
		args = append(args, "/remove:d", "*"+sid)
	}
	args = append(args, "/remove:d", "*S-1-1-0", "/c", "/q", "/l")
	removed := run("icacls", args...)
	granted := run("icacls", path, "/grant", "*S-1-5-32-544:F", "/c", "/q", "/l")
	return removed || granted
}

// The shell belongs to the process account. Cross-account elevation must not
// empty that administrator's recycle bin instead of the original user's bin.
func currentUID() string        { return "" }
func IsVolumeTrash(string) bool { return false }
func EmptyTrashShell() error {
	real, hasReal := UserSID()
	process, hasProcess := fileopsProcessSID()
	if !hasReal || !hasProcess || real != process {
		return nil
	}
	hr, _, _ := shellEmptyRecycle.Call(0, 0, 7)
	if uint32(hr) != 0 && uint32(hr) != 0x8000FFFF {
		return fmt.Errorf("SHEmptyRecycleBinW: 0x%08X", uint32(hr))
	}
	return nil
}

func trashDirectories() []string {
	sid, ok := UserSID()
	if !ok || !strings.HasPrefix(sid, "S-1-") {
		return nil
	}
	for _, r := range sid {
		if r != 'S' && r != '-' && (r < '0' || r > '9') {
			return nil
		}
	}
	var dirs []string
	for drive := 'A'; drive <= 'Z'; drive++ {
		dirs = append(dirs, filepath.Join(string(drive)+`:\$Recycle.Bin`, sid))
	}
	return dirs
}

func IsSystemTrash(path string) bool { return strings.Contains(strings.ToLower(path), "$recycle.bin") }

// IsPerDriveTrash reports one drive's recycle bin for the original user.
// Emptying the shell bin once does not cover every drive, so each listed bin
// is emptied on its own.
func IsPerDriveTrash(path string) bool {
	sid, ok := UserSID()
	if !ok {
		return false
	}
	return strings.EqualFold(filepath.Base(path), sid) && strings.EqualFold(filepath.Base(filepath.Dir(path)), "$Recycle.Bin")
}
func IsRecycleJunkEntry(name string) bool       { return !strings.EqualFold(name, "desktop.ini") }
func AllocatedFileSize(info os.FileInfo) uint64 { return logicalFileSize(info) }
