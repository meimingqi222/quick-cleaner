package platform

import (
	"errors"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"
)

// RunningAppOwners lists this user's live processes whose image name is in
// names. It reads the process list only (no file probing), so it is cheap
// enough for scan-time badges. Processes the cache-release path would refuse
// to ask to quit (other users, services, critical processes, Explorer) are
// omitted rather than reported as errors: they are not the user's to close.
func RunningAppOwners(names []string) ([]CacheOccupant, error) {
	want := map[string]bool{}
	for _, name := range names {
		want[strings.ToLower(name)] = true
	}
	if len(want) == 0 {
		return nil, nil
	}
	snap, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return nil, err
	}
	defer windows.CloseHandle(snap)
	var entry windows.ProcessEntry32
	entry.Size = uint32(unsafe.Sizeof(entry))
	var out []CacheOccupant
	for err = windows.Process32First(snap, &entry); err == nil; err = windows.Process32Next(snap, &entry) {
		name := windows.UTF16ToString(entry.ExeFile[:])
		if !want[strings.ToLower(name)] {
			continue
		}
		if owner, ok := appOwner(entry.ProcessID, name); ok {
			out = append(out, owner)
		}
	}
	if !errors.Is(err, windows.ERROR_NO_MORE_FILES) {
		return nil, err
	}
	return out, nil
}

func appOwner(pid uint32, name string) (CacheOccupant, bool) {
	h, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return CacheOccupant{}, false
	}
	defer windows.CloseHandle(h)
	var created, exited, kernel, user windows.Filetime
	if windows.GetProcessTimes(h, &created, &exited, &kernel, &user) != nil {
		return CacheOccupant{}, false
	}
	buf := make([]uint16, windows.MAX_LONG_PATH)
	size := uint32(len(buf))
	if windows.QueryFullProcessImageName(h, 0, &buf[0], &size) != nil {
		return CacheOccupant{}, false
	}
	owner := CacheOccupant{PID: pid, Identity: cacheFiletime(created), Executable: windows.UTF16ToString(buf[:size]), Name: name}
	if cacheVerifyHandle(h, owner, true) != nil {
		return CacheOccupant{}, false
	}
	return owner, true
}
