package platform

import (
	"errors"
	"os"
	"unsafe"

	"golang.org/x/sys/windows"
)

func cacheOutsideCleanerTree(pid uint32) error {
	snapshot, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
	if err != nil {
		return err
	}
	defer windows.CloseHandle(snapshot)
	parents := map[uint32]uint32{}
	entry := windows.ProcessEntry32{Size: uint32(unsafe.Sizeof(windows.ProcessEntry32{}))}
	for err = windows.Process32First(snapshot, &entry); err == nil; err = windows.Process32Next(snapshot, &entry) {
		parents[entry.ProcessID] = entry.ParentProcessID
	}
	if !errors.Is(err, windows.ERROR_NO_MORE_FILES) {
		return err
	}
	return cacheCheckAncestry(pid, uint32(os.Getpid()), parents)
}

func cacheCheckAncestry(pid, cleaner uint32, parents map[uint32]uint32) error {
	if _, ok := parents[pid]; !ok {
		return errors.New("owner-exited; retry")
	}
	for depth := 0; ; depth++ {
		if depth >= 256 || pid == cleaner {
			return errors.New("cleaner-process-tree-or-unknown-ancestry")
		}
		parent, present := parents[pid]
		// Windows retains the creator PID after it exits. Only a complete
		// snapshot proves that ancestor absent; unreadable snapshots fail above.
		if !present || parent <= 1 {
			return nil
		}
		pid = parent
	}
}
