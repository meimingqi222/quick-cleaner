package platform

import "syscall"

func ReleaseMemory() bool {
	handle, err := syscall.GetCurrentProcess()
	if err != nil {
		return false
	}
	ok, _, _ := syscall.NewLazyDLL("psapi.dll").NewProc("EmptyWorkingSet").Call(uintptr(handle))
	return ok != 0
}
