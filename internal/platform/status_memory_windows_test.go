package platform

import (
	"os"
	"testing"
	"unsafe"
)

func TestStatusMemoryWindowsABI(t *testing.T) {
	if unsafe.Sizeof(uintptr(0)) != 8 {
		t.Skip("64-bit ABI checks")
	}
	var ram statusMemoryEx
	var perf statusPerformance
	var proc statusProcessMemoryEx
	if unsafe.Sizeof(ram) != 64 || unsafe.Sizeof(perf) != 104 || unsafe.Sizeof(proc) != 80 || unsafe.Offsetof(proc.PrivateUsage) != 72 {
		t.Fatal("Windows memory ABI mismatch")
	}
}

func TestStatusMemoryWindowsOwnPrivateUsage(t *testing.T) {
	value := ProcessVirtualMemory(uint32(os.Getpid()))
	if value == nil || *value == 0 {
		t.Fatal("own private usage unavailable")
	}
	if ProcessVirtualMemory(0) != nil {
		t.Fatal("PID 0 must not produce memory data")
	}
}
