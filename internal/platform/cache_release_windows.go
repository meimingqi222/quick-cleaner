package platform

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var cacheRM = windows.NewLazySystemDLL("rstrtmgr.dll")
var cacheRMStart = cacheRM.NewProc("RmStartSession")
var cacheRMEnd = cacheRM.NewProc("RmEndSession")
var cacheRMRegister = cacheRM.NewProc("RmRegisterResources")
var cacheRMList = cacheRM.NewProc("RmGetList")
var cacheUser32 = windows.NewLazySystemDLL("user32.dll")
var cacheEnumWindows = cacheUser32.NewProc("EnumWindows")
var cacheWindowPID = cacheUser32.NewProc("GetWindowThreadProcessId")
var cachePostMessage = cacheUser32.NewProc("PostMessageW")
var cacheIsCritical = windows.NewLazySystemDLL("kernel32.dll").NewProc("IsProcessCritical")

type cacheCloseRequest struct {
	handle windows.Handle
	owner  CacheOccupant
	err    error
}

var cacheCloseRequests sync.Map
var cacheCloseSequence atomic.Uint64

// Windows callbacks are never freed by the Go runtime, so allocate one rather
// than retaining a fresh closure and process handle for every Quit request.
var cacheCloseCallback = syscall.NewCallback(func(window, context uintptr) uintptr {
	value, ok := cacheCloseRequests.Load(context)
	if !ok {
		return 0
	}
	r := value.(*cacheCloseRequest)
	var pid uint32
	cacheWindowPID.Call(window, uintptr(unsafe.Pointer(&pid)))
	if pid == r.owner.PID {
		if err := cacheVerifyHandle(r.handle, r.owner, true); err != nil {
			r.err = err
			return 0
		}
		ok, _, err := cachePostMessage.Call(window, 0x10, 0, 0)
		if ok == 0 {
			r.err = err
			return 0
		}
	}
	return 1
})

type cacheRMInfo struct {
	PID                                uint32
	Created                            windows.Filetime
	AppName                            [256]uint16
	Service                            [64]uint16
	Type, Status, Session, Restartable uint32
}

var _ [668 - unsafe.Sizeof(cacheRMInfo{})]byte
var _ [unsafe.Sizeof(cacheRMInfo{}) - 668]byte

func cacheFiletime(t windows.Filetime) uint64 {
	return uint64(t.HighDateTime)<<32 | uint64(t.LowDateTime)
}

func CacheReleaseOccupants(path string) ([]CacheOccupant, error) {
	if !filepath.IsAbs(path) {
		return nil, errors.New("cache-path-not-absolute")
	}
	if err := appRejectStagingLinks(path); err != nil {
		return nil, err
	}
	var session uint32
	var key [33]uint16
	code, _, _ := cacheRMStart.Call(uintptr(unsafe.Pointer(&session)), 0, uintptr(unsafe.Pointer(&key[0])))
	if code != 0 {
		return nil, fmt.Errorf("RmStartSession=%d", code)
	}
	defer cacheRMEnd.Call(uintptr(session))
	deadline := time.Now().Add(30 * time.Second)
	count := 0
	var files []*uint16
	err := filepath.WalkDir(path, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		count++
		if count > 20000 || !time.Now().Before(deadline) {
			return errors.New("cache-owner-probe-budget-exceeded")
		}
		ptr, err := windows.UTF16PtrFromString(p)
		if err != nil {
			return err
		}
		attr, err := windows.GetFileAttributes(ptr)
		if err != nil {
			return err
		}
		if d.Type()&os.ModeSymlink != 0 || attr&windows.FILE_ATTRIBUTE_REPARSE_POINT != 0 {
			return errors.New("cache-owner-probe-link")
		}
		if !d.IsDir() {
			info, err := d.Info()
			if err != nil {
				return err
			}
			if !info.Mode().IsRegular() {
				return errors.New("cache-owner-probe-special-file")
			}
			files = append(files, ptr)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	for i := 0; i < len(files); i += 256 {
		if !time.Now().Before(deadline) {
			return nil, errors.New("cache-owner-probe-budget-exceeded")
		}
		batch := files[i:min(i+256, len(files))]
		code, _, _ = cacheRMRegister.Call(uintptr(session), uintptr(len(batch)), uintptr(unsafe.Pointer(&batch[0])), 0, 0, 0, 0)
		if code != 0 {
			return nil, fmt.Errorf("RmRegisterResources=%d", code)
		}
	}
	runtime.KeepAlive(files)
	var needed, n, reboot uint32
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), 0, uintptr(unsafe.Pointer(&reboot)))
	if !time.Now().Before(deadline) {
		return nil, errors.New("cache-owner-probe-budget-exceeded")
	}
	if code == 0 {
		if n != 0 || needed != 0 || reboot != 0 {
			return nil, errors.New("cache-owner-probe-incomplete")
		}
		return []CacheOccupant{}, nil
	}
	if code != 234 || needed == 0 || needed > 128 {
		return nil, fmt.Errorf("RmGetList=%d; needed=%d", code, needed)
	}
	infos := make([]cacheRMInfo, needed)
	n = needed
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), uintptr(unsafe.Pointer(&infos[0])), uintptr(unsafe.Pointer(&reboot)))
	if code != 0 || int(n) > len(infos) || reboot != 0 {
		return nil, fmt.Errorf("RmGetList=%d; incomplete-owner-list", code)
	}
	owners := map[uint32]CacheOccupant{}
	for _, info := range infos[:n] {
		if !time.Now().Before(deadline) {
			return nil, errors.New("cache-owner-probe-budget-exceeded")
		}
		if info.Type == 3 || info.Type == 4 || info.Type == 1000 || info.Service[0] != 0 {
			return nil, errors.New("service-or-critical-owner")
		}
		owner := CacheOccupant{PID: info.PID, Identity: cacheFiletime(info.Created), Evidence: path}
		h, err := cacheOpenVerified(owner, false, false)
		if err != nil {
			return nil, err
		}
		owner.Name, owner.Executable, err = cacheProcessSnapshot(info.PID)
		if err == nil {
			err = cacheVerifyHandle(h, owner, true)
		}
		windows.CloseHandle(h)
		if err != nil {
			return nil, err
		}
		if old, ok := owners[owner.PID]; ok && (old.Identity != owner.Identity || old.Executable != owner.Executable) {
			return nil, errors.New("owner-identity-changed")
		}
		owners[owner.PID] = owner
	}
	if len(owners) > 16 {
		return nil, errors.New("too-many-cache-owners")
	}
	out := make([]CacheOccupant, 0, len(owners))
	for _, o := range owners {
		out = append(out, o)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].PID < out[j].PID })
	return out, nil
}

func cacheVerifyHandle(h windows.Handle, owner CacheOccupant, checkExe bool) error {
	var created, exited, kernel, user windows.Filetime
	if err := windows.GetProcessTimes(h, &created, &exited, &kernel, &user); err != nil {
		return err
	}
	if owner.Identity == 0 || cacheFiletime(created) != owner.Identity {
		return errors.New("owner-identity-changed")
	}
	var critical int32
	ok, _, err := cacheIsCritical.Call(uintptr(h), uintptr(unsafe.Pointer(&critical)))
	if ok == 0 {
		return fmt.Errorf("owner-critical-status-unknown: %w", err)
	}
	if critical != 0 {
		return errors.New("critical-owner")
	}
	var status uint32
	if err := windows.GetExitCodeProcess(h, &status); err != nil {
		return err
	}
	if status != 259 {
		return errors.New("owner-exited; retry")
	}
	var token windows.Token
	if err := windows.OpenProcessToken(h, windows.TOKEN_QUERY, &token); err != nil {
		return err
	}
	defer token.Close()
	me, err := windows.OpenCurrentProcessToken()
	if err != nil {
		return err
	}
	defer me.Close()
	u, err := token.GetTokenUser()
	if err != nil {
		return err
	}
	m, err := me.GetTokenUser()
	if err != nil {
		return err
	}
	if !u.User.Sid.Equals(m.User.Sid) || u.User.Sid.IsWellKnown(windows.WinLocalSystemSid) || u.User.Sid.IsWellKnown(windows.WinLocalServiceSid) || u.User.Sid.IsWellKnown(windows.WinNetworkServiceSid) {
		return errors.New("system-or-other-user-process")
	}
	if checkExe {
		buf := make([]uint16, 32768)
		size := uint32(len(buf))
		if err := windows.QueryFullProcessImageName(h, 0, &buf[0], &size); err != nil {
			return err
		}
		exe := windows.UTF16ToString(buf[:size])
		if exe != owner.Executable || strings.EqualFold(filepath.Base(exe), "explorer.exe") {
			return errors.New("owner-executable-changed-or-protected")
		}
	}
	return nil
}

func cacheOpenVerified(owner CacheOccupant, force, checkExe bool) (windows.Handle, error) {
	access := uint32(windows.PROCESS_QUERY_LIMITED_INFORMATION)
	if force {
		access |= windows.PROCESS_TERMINATE
	}
	h, err := windows.OpenProcess(access, false, owner.PID)
	if err != nil {
		return 0, err
	}
	if err := cacheVerifyHandle(h, owner, checkExe); err != nil {
		windows.CloseHandle(h)
		return 0, err
	}
	return h, nil
}

func RequestCacheOwnerExit(owner CacheOccupant, force bool) error {
	_, exe, err := cacheProcessSnapshot(owner.PID)
	if err != nil {
		return err
	}
	if exe != owner.Executable {
		return errors.New("owner-executable-changed")
	}
	h, err := cacheOpenVerified(owner, force, true)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(h)
	if force {
		return windows.TerminateProcess(h, 1)
	}
	request := cacheCloseRequest{handle: h, owner: owner}
	key := uintptr(cacheCloseSequence.Add(1))
	cacheCloseRequests.Store(key, &request)
	defer cacheCloseRequests.Delete(key)
	ok, _, enumErr := cacheEnumWindows.Call(cacheCloseCallback, key)
	if request.err != nil {
		return request.err
	}
	if ok == 0 {
		return enumErr
	}
	return nil
}
