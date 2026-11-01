package platform

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"github.com/shirou/gopsutil/v4/process"
	"golang.org/x/sys/windows"
)

const (
	occupancyMaxEntries = 8192
	occupancyDeadline   = 3 * time.Second
	occupancyMaxLocks   = 32
	occupancyMaxOwners  = 16
)

// occupancyOwners walks until it finds locked files, then asks Restart Manager
// who holds them. It reports system owners instead of refusing the whole probe,
// because the residual panel must explain a skipped system lock. Termination
// authority stays in the strict cache-release path.
func occupancyOwners(root string) ([]Occupant, error) {
	if !filepath.IsAbs(root) {
		return nil, errors.New("occupancy-path-not-absolute")
	}
	md, err := os.Lstat(root)
	if err != nil {
		return nil, err
	}
	if md.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("occupancy-link")
	}
	var locked []string
	if md.IsDir() {
		locked, err = occupancyLockedFiles(root)
		if err != nil {
			return nil, err
		}
	} else {
		if !md.Mode().IsRegular() {
			return nil, errors.New("occupancy-special-file")
		}
		busy, known := inUseFileOpen(root)
		if !known {
			return nil, errors.New("occupancy-unknown")
		}
		if busy {
			locked = []string{root}
		}
	}
	if len(locked) == 0 {
		return nil, nil
	}
	return occupancyRestartManager(locked)
}

func occupancyLockedFiles(root string) ([]string, error) {
	locked, known := scanDirLocks(root, occupancyMaxEntries, occupancyDeadline, occupancyMaxLocks)
	if len(locked) == 0 {
		if !known {
			return nil, errors.New("occupancy-unknown")
		}
		return nil, nil
	}
	if len(locked) > occupancyMaxLocks {
		locked = locked[:occupancyMaxLocks]
	}
	return locked, nil
}

func occupancyRestartManager(files []string) ([]Occupant, error) {
	var session uint32
	var key [33]uint16
	code, _, _ := cacheRMStart.Call(uintptr(unsafe.Pointer(&session)), 0, uintptr(unsafe.Pointer(&key[0])))
	if code != 0 {
		return nil, fmt.Errorf("RmStartSession=%d", code)
	}
	defer cacheRMEnd.Call(uintptr(session))
	ptrs := make([]*uint16, 0, len(files))
	for _, f := range files {
		p, err := windows.UTF16PtrFromString(f)
		if err != nil {
			return nil, err
		}
		ptrs = append(ptrs, p)
	}
	code, _, _ = cacheRMRegister.Call(uintptr(session), uintptr(len(ptrs)), uintptr(unsafe.Pointer(&ptrs[0])), 0, 0, 0, 0)
	runtime.KeepAlive(ptrs)
	if code != 0 {
		return nil, fmt.Errorf("RmRegisterResources=%d", code)
	}
	var needed, n, reboot uint32
	code, _, _ = cacheRMList.Call(uintptr(session), uintptr(unsafe.Pointer(&needed)), uintptr(unsafe.Pointer(&n)), 0, uintptr(unsafe.Pointer(&reboot)))
	if code == 0 {
		if n != 0 || needed != 0 || reboot != 0 {
			return nil, errors.New("occupancy-incomplete")
		}
		return nil, nil
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
	seen := map[uint32]bool{}
	out := make([]Occupant, 0, len(infos))
	for _, info := range infos[:n] {
		if info.PID == 0 || seen[info.PID] {
			continue
		}
		seen[info.PID] = true
		name, exe, system := occupancyOwner(info)
		out = append(out, Occupant{PID: info.PID, Name: name, Executable: exe, System: system})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].PID < out[j].PID })
	if len(out) > occupancyMaxOwners {
		out = out[:occupancyMaxOwners]
	}
	return out, nil
}

func occupancyOwner(info cacheRMInfo) (name, executable string, system bool) {
	app := windows.UTF16ToString(info.AppName[:])
	service := windows.UTF16ToString(info.Service[:])
	if service != "" || info.Type == 3 || info.Type == 4 || info.Type == 1000 {
		return occupancyFallbackName(app, info.PID), "", true
	}
	var ownerSession, ownSession uint32
	if windows.ProcessIdToSessionId(info.PID, &ownerSession) == nil &&
		windows.ProcessIdToSessionId(uint32(os.Getpid()), &ownSession) == nil && ownerSession != ownSession {
		return occupancyFallbackName(app, info.PID), "", true
	}
	p, err := process.NewProcess(int32(info.PID))
	if err != nil {
		return occupancyFallbackName(app, info.PID), "", true
	}
	user, err := p.Username()
	me, meErr := process.NewProcess(int32(os.Getpid()))
	if err != nil || user == "" || meErr != nil {
		return occupancyFallbackName(app, info.PID), "", true
	}
	own, err := me.Username()
	if err != nil || !strings.EqualFold(user, own) {
		return occupancyFallbackName(app, info.PID), "", true
	}
	exe, exeErr := p.Exe()
	if exeErr == nil {
		executable = exe
		if strings.EqualFold(filepath.Base(exe), "explorer.exe") {
			return occupancyFallbackName(app, info.PID), executable, true
		}
	}
	if app != "" {
		return app, executable, false
	}
	if name, err := p.Name(); err == nil && name != "" {
		return name, executable, false
	}
	if executable != "" {
		return filepath.Base(executable), executable, false
	}
	return occupancyFallbackName(app, info.PID), executable, false
}

func occupancyFallbackName(app string, pid uint32) string {
	if app != "" {
		return app
	}
	return "PID " + strconv.Itoa(int(pid))
}
